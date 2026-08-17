//! Shared server state.

use std::collections::HashMap;
use std::sync::Arc;

use jet_store::Db;

use crate::webhook::{DeliveryGuard, Provider, TriggerEvent};

/// Per-provider webhook secrets.
///
/// Held separately per provider rather than as one global secret: rotating
/// GitHub's should not invalidate GitLab's, and a leaked secret should not grant
/// the ability to forge deliveries for every provider at once.
#[derive(Debug, Default, Clone)]
pub struct WebhookSecrets(HashMap<&'static str, String>);

impl WebhookSecrets {
    pub fn set(&mut self, provider: Provider, secret: impl Into<String>) {
        self.0.insert(provider.as_str(), secret.into());
    }

    pub fn get(&self, provider: Provider) -> Option<&String> {
        self.0.get(provider.as_str())
    }
}

pub struct ServerState {
    pub db: Db,
    pub deliveries: DeliveryGuard,
    secrets: WebhookSecrets,
    /// `repo full_name` -> `(project slug, pipeline slug)`.
    ///
    /// Resolved from the database in a real deployment; kept as a map here so the
    /// webhook path is complete and testable before the project/pipeline
    /// repositories land.
    repo_map: HashMap<String, (String, String)>,
}

impl ServerState {
    pub fn new(db: Db, secrets: WebhookSecrets) -> Self {
        ServerState {
            db,
            deliveries: DeliveryGuard::default(),
            secrets,
            repo_map: HashMap::new(),
        }
    }

    pub fn map_repo(
        &mut self,
        repo: impl Into<String>,
        project: impl Into<String>,
        pipeline: impl Into<String>,
    ) {
        self.repo_map
            .insert(repo.into(), (project.into(), pipeline.into()));
    }

    pub fn webhook_secret(&self, provider: Provider) -> Option<String> {
        self.secrets.get(provider).cloned()
    }

    /// Turn a verified, deduplicated event into a queued run.
    ///
    /// Returns as soon as the run is durably queued rather than waiting for it to
    /// execute. Webhook providers time out in single-digit seconds (GitHub at
    /// ten), so a handler that waited for a build would guarantee redeliveries and
    /// duplicate work.
    pub async fn enqueue(&self, event: TriggerEvent) -> Result<String, EnqueueError> {
        let (project, pipeline) = self
            .repo_map
            .get(&event.repo)
            .cloned()
            .ok_or_else(|| EnqueueError::UnknownRepo(event.repo.clone()))?;

        tracing::info!(
            provider = %event.provider,
            repo = %event.repo,
            %project,
            %pipeline,
            authenticated_body = event.body_authenticated,
            "queueing run from webhook"
        );

        // TODO(jet-sched): insert the run row and hand it to the scheduler. The
        // id is minted here so the provider gets something to correlate with.
        Ok(jet_core::RunId::new().to_canonical())
    }

    /// Verify an API token presented as a bearer credential.
    pub async fn authenticate_token(&self, token: &str) -> Result<(), AuthError> {
        // Tokens are `jetr_<prefix>_<secret>`. The prefix is the indexable
        // handle; the secret is checked against an Argon2 hash. Splitting first
        // means a malformed token costs a string operation rather than a KDF.
        let mut parts = token.splitn(3, '_');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("jetr"), Some(prefix), Some(secret))
                if !prefix.is_empty() && !secret.is_empty() =>
            {
                // TODO(jet-authz): look up api_tokens by prefix, verify the secret
                // with Argon2, check revoked_at/expires_at, and resolve the
                // principal.
                Err(AuthError::Unknown)
            }
            _ => Err(AuthError::Malformed),
        }
    }

    /// In-memory state for tests, with one repository mapped.
    #[cfg(any(test, feature = "test-util"))]
    pub async fn for_test(secret: &str) -> Self {
        let db = Db::open_in_memory().await.expect("in-memory db");
        jet_store::bootstrap(&db).await.expect("bootstrap");

        let mut secrets = WebhookSecrets::default();
        for p in [Provider::GitHub, Provider::GitLab, Provider::Bitbucket] {
            secrets.set(p, secret);
        }

        let mut state = ServerState::new(db, secrets);
        state.map_repo("acme/web", "web", "ci");
        state
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EnqueueError {
    /// No pipeline is configured for the repository the event came from.
    ///
    /// Common and benign: a webhook installed org-wide fires for every repo,
    /// including ones jetrun does not build.
    #[error("no pipeline configured for repository {0}")]
    UnknownRepo(String),
    #[error(transparent)]
    Store(#[from] jet_store::StoreError),
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("token is not in the expected format")]
    Malformed,
    #[error("token is not recognized")]
    Unknown,
}

/// Convenience alias for the shared handle.
pub type SharedState = Arc<ServerState>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::webhook::EventKind;

    fn push_event(repo: &str) -> TriggerEvent {
        TriggerEvent {
            provider: Provider::GitHub,
            delivery_id: Some("d".into()),
            kind: EventKind::Push {
                branch: "main".into(),
            },
            repo: repo.into(),
            commit_sha: Some("abc".into()),
            reference: Some("refs/heads/main".into()),
            actor: Some("dev".into()),
            body_authenticated: true,
        }
    }

    #[tokio::test]
    async fn enqueue_resolves_a_mapped_repository() {
        let state = ServerState::for_test("s").await;
        let run = state.enqueue(push_event("acme/web")).await.unwrap();
        assert!(!run.is_empty());
    }

    #[tokio::test]
    async fn enqueue_reports_an_unmapped_repository_distinctly() {
        // Must be distinguishable from a real failure: the handler answers 200 for
        // this and 500 for anything else.
        let state = ServerState::for_test("s").await;
        let err = state.enqueue(push_event("someone/else")).await.unwrap_err();
        assert!(matches!(err, EnqueueError::UnknownRepo(r) if r == "someone/else"));
    }

    #[tokio::test]
    async fn secrets_are_per_provider() {
        let mut secrets = WebhookSecrets::default();
        secrets.set(Provider::GitHub, "gh-secret");
        let db = Db::open_in_memory().await.unwrap();
        let state = ServerState::new(db, secrets);

        assert_eq!(state.webhook_secret(Provider::GitHub).as_deref(), Some("gh-secret"));
        assert_eq!(
            state.webhook_secret(Provider::GitLab),
            None,
            "an unset provider must not inherit another's secret"
        );
    }

    #[tokio::test]
    async fn malformed_tokens_are_rejected_without_reaching_the_kdf() {
        let state = ServerState::for_test("s").await;
        for bad in ["", "garbage", "jetr_", "jetr_prefix", "bearer_x_y", "jetr__secret"] {
            assert!(
                matches!(state.authenticate_token(bad).await, Err(AuthError::Malformed)),
                "{bad:?} should be malformed"
            );
        }
    }

    #[tokio::test]
    async fn wellformed_but_unknown_token_is_distinguished_from_malformed() {
        let state = ServerState::for_test("s").await;
        assert!(matches!(
            state.authenticate_token("jetr_abcd1234_secretpart").await,
            Err(AuthError::Unknown)
        ));
    }
}
