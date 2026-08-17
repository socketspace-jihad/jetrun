//! The public HTTP/JSON listener.
//!
//! Deliberately *not* JRP. Webhook providers, browsers, and third-party
//! integrations all speak HTTP/JSON and cannot be asked to speak anything else,
//! so inventing a protocol on this surface would buy nothing and cost every
//! integration. JRP earns its keep on the internal path, where the traffic volume
//! and the bulk CAS transfer are.
//!
//! # Routes
//!
//! ```text
//! GET  /healthz                   liveness, unauthenticated
//! POST /webhooks/github           HMAC-SHA256 signed
//! POST /webhooks/gitlab           shared-secret token
//! POST /webhooks/bitbucket        HMAC-SHA256 signed
//! POST /api/v1/trigger            jetrun API token (Bearer)
//! ```
//!
//! The last one is the migration path: a GitHub Actions job, GitLab CI job, or
//! Bitbucket Pipeline that wants to hand work to jetrun authenticates as a jetrun
//! service account rather than being signature-verified, because those systems do
//! not sign anything on our behalf. Keeping it separate from the webhook routes
//! is what stops "we support triggering from CI" from becoming "we accept
//! unsigned requests".

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Serialize;

use crate::state::ServerState;
use crate::webhook::{
    self, Delivery, EventKind, MAX_BODY_BYTES, Provider, TriggerEvent, VerifyError, WebhookError,
};

pub fn router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/webhooks/{provider}", post(webhook))
        .route("/api/v1/trigger", post(trigger))
        // Applied to the whole router: the body cap must bite before any handler
        // runs, because HMAC over an unbounded body is itself the DoS.
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

#[derive(Serialize)]
struct Ack {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
}

impl Ack {
    fn accepted(run_id: String) -> Response {
        (
            StatusCode::ACCEPTED,
            axum::Json(Ack {
                status: "accepted",
                detail: None,
                run_id: Some(run_id),
            }),
        )
            .into_response()
    }

    /// Acknowledged without starting a run.
    ///
    /// Returned as 200, never as an error. Providers disable endpoints that keep
    /// failing, so "I understood you and chose to do nothing" must not look like
    /// a failure -- otherwise a repository full of `star` events eventually
    /// switches off CI for the whole org.
    fn ignored(detail: impl Into<String>) -> Response {
        (
            StatusCode::OK,
            axum::Json(Ack {
                status: "ignored",
                detail: Some(detail.into()),
                run_id: None,
            }),
        )
            .into_response()
    }
}

/// Lowercased header map, for the provider-independent verifier.
struct Headers(HashMap<String, String>);

impl Headers {
    fn from(map: &HeaderMap) -> Self {
        Headers(
            map.iter()
                .filter_map(|(k, v)| {
                    v.to_str()
                        .ok()
                        .map(|v| (k.as_str().to_ascii_lowercase(), v.to_owned()))
                })
                .collect(),
        )
    }
}

impl webhook::HeaderLookup for Headers {
    fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

async fn webhook(
    State(state): State<Arc<ServerState>>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    // `Bytes`, not `Json`. The signature covers the exact bytes received;
    // deserializing first and re-serializing to verify can never match, and
    // "verify afterwards" reliably becomes "verify never".
    body: Bytes,
) -> Response {
    let provider = match provider.as_str() {
        "github" => Provider::GitHub,
        "gitlab" => Provider::GitLab,
        "bitbucket" => Provider::Bitbucket,
        other => {
            return (
                StatusCode::NOT_FOUND,
                format!("unknown webhook provider {other:?}"),
            )
                .into_response();
        }
    };

    let lookup = Headers::from(&headers);

    // 1. Authenticate before parsing. A failure here is deliberately terse: the
    //    caller is unauthenticated and gets no detail about why.
    let verified = match webhook::verify::verify(
        provider,
        &lookup,
        &body,
        state.webhook_secret(provider).as_deref().map(|s| s.as_bytes()),
    ) {
        Ok(v) => v,
        Err(e) => {
            let status = match e {
                // A missing secret is *our* misconfiguration, not the caller's
                // fault, and it should page an operator rather than look like a
                // rejected request.
                VerifyError::NoSecretConfigured(_) => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::UNAUTHORIZED,
            };
            tracing::warn!(%provider, error = %e, "webhook rejected");
            return (status, "rejected").into_response();
        }
    };

    // 2. Reject replays. A redelivery is a byte-identical, correctly signed
    //    request, so the signature cannot distinguish it -- only the delivery id
    //    can.
    let delivery_id = lookup_delivery(&lookup, provider);
    match state.deliveries.check(delivery_id.as_deref()) {
        Delivery::Duplicate => {
            tracing::info!(%provider, id = ?delivery_id, "duplicate delivery ignored");
            return Ack::ignored("duplicate delivery");
        }
        Delivery::Unidentified => {
            tracing::debug!(%provider, "delivery carried no id; replay protection unavailable");
        }
        Delivery::Fresh => {}
    }

    // 3. Now it is safe to parse.
    let event = match webhook::parse(provider, &lookup, &body, verified) {
        Ok(e) => e,
        Err(WebhookError::MissingHeader(h)) => {
            return (StatusCode::BAD_REQUEST, format!("missing {h}")).into_response();
        }
        Err(e) => {
            tracing::warn!(%provider, error = %e, "webhook payload not understood");
            return (StatusCode::BAD_REQUEST, "malformed payload").into_response();
        }
    };

    dispatch(&state, event).await
}

fn lookup_delivery(lookup: &Headers, provider: Provider) -> Option<String> {
    webhook::HeaderLookup::get(lookup, provider.delivery_header()).map(str::to_owned)
}

async fn dispatch(state: &ServerState, event: TriggerEvent) -> Response {
    if !event.should_trigger() {
        let detail = match &event.kind {
            EventKind::Ping => "ping acknowledged".to_string(),
            EventKind::Ignored { event } => format!("event {event} is not configured to trigger"),
            EventKind::PullRequest { action, .. } => {
                format!("pull request action {action} does not trigger")
            }
            _ => "not configured to trigger".to_string(),
        };
        return Ack::ignored(detail);
    }

    match state.enqueue(event).await {
        Ok(run_id) => Ack::accepted(run_id),
        // An unmapped repository is not an error the provider should retry, and
        // answering 404 would also confirm which repositories exist to anyone who
        // holds the webhook secret. Acknowledge and log.
        Err(crate::state::EnqueueError::UnknownRepo(repo)) => {
            tracing::info!(%repo, "webhook for a repository with no configured pipeline");
            Ack::ignored("no pipeline configured for this repository")
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to enqueue run");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

/// Generic trigger for an existing CI system handing work to jetrun.
///
/// Authenticated with a jetrun API token, because GitHub Actions and friends do
/// not sign requests on our behalf. This is the migration on-ramp: keep your
/// existing workflow, delegate the slow job here.
async fn trigger(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(token) = bearer(&headers) else {
        return (StatusCode::UNAUTHORIZED, "missing bearer token").into_response();
    };
    if token.len() > jet_proto::limits::MAX_TOKEN_LEN {
        // Bounded before it reaches Argon2, same reasoning as the JRP path.
        return (StatusCode::BAD_REQUEST, "token too long").into_response();
    }

    match state.authenticate_token(token).await {
        Ok(_principal) => {}
        Err(e) => {
            tracing::warn!(error = %e, "api trigger rejected");
            return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
        }
    }

    #[derive(serde::Deserialize)]
    struct TriggerBody {
        project: String,
        pipeline: String,
        #[serde(default)]
        commit_sha: Option<String>,
        #[serde(default)]
        reference: Option<String>,
    }

    let req: TriggerBody = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("bad body: {e}")).into_response(),
    };

    let event = TriggerEvent {
        provider: Provider::GitHub, // placeholder; API triggers carry no provider
        delivery_id: None,
        kind: EventKind::Push {
            branch: req
                .reference
                .clone()
                .unwrap_or_else(|| "api".to_string()),
        },
        repo: format!("{}/{}", req.project, req.pipeline),
        commit_sha: req.commit_sha,
        reference: req.reference,
        actor: None,
        // The request was authenticated by token over TLS, not signed.
        body_authenticated: false,
    };

    match state.enqueue(event).await {
        Ok(run_id) => Ack::accepted(run_id),
        Err(e) => {
            tracing::error!(error = %e, "api trigger failed");
            (StatusCode::BAD_REQUEST, e.to_string()).into_response()
        }
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ServerState;
    use crate::webhook::sign_hmac_sha256;

    const SECRET: &str = "webhook-secret";

    async fn app() -> Router {
        let state = ServerState::for_test(SECRET).await;
        router(Arc::new(state))
    }

    /// Drive a request through the real router, so ordering and layers are
    /// exercised rather than the handler in isolation.
    async fn send(app: &Router, req: axum::http::Request<axum::body::Body>) -> (StatusCode, String) {
        use tower::ServiceExt;
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).to_string())
    }

    fn github_push(body: &str, sig: Option<&str>, delivery: &str) -> axum::http::Request<axum::body::Body> {
        let mut b = axum::http::Request::builder()
            .method("POST")
            .uri("/webhooks/github")
            .header("X-GitHub-Event", "push")
            .header("X-GitHub-Delivery", delivery);
        if let Some(s) = sig {
            b = b.header("X-Hub-Signature-256", s);
        }
        b.body(axum::body::Body::from(body.to_owned())).unwrap()
    }

    const PUSH: &str = r#"{"ref":"refs/heads/main","after":"abc","repository":{"full_name":"acme/web"},"pusher":{"name":"dev"}}"#;

    #[tokio::test]
    async fn healthz_is_open() {
        let app = app().await;
        let req = axum::http::Request::builder()
            .uri("/healthz")
            .body(axum::body::Body::empty())
            .unwrap();
        let (status, body) = send(&app, req).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "ok");
    }

    #[tokio::test]
    async fn signed_github_push_is_accepted() {
        let app = app().await;
        let sig = sign_hmac_sha256(PUSH.as_bytes(), SECRET.as_bytes());
        let (status, body) = send(&app, github_push(PUSH, Some(&sig), "d1")).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        assert!(body.contains("accepted"), "{body}");
    }

    #[tokio::test]
    async fn unsigned_request_is_rejected() {
        let app = app().await;
        let (status, _) = send(&app, github_push(PUSH, None, "d2")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_signature_is_rejected() {
        let app = app().await;
        let sig = sign_hmac_sha256(PUSH.as_bytes(), b"not-the-secret");
        let (status, _) = send(&app, github_push(PUSH, Some(&sig), "d3")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn tampered_body_is_rejected() {
        // Signature computed over the original, body swapped in flight.
        let app = app().await;
        let sig = sign_hmac_sha256(PUSH.as_bytes(), SECRET.as_bytes());
        let evil = PUSH.replace("refs/heads/main", "refs/heads/evil");
        let (status, _) = send(&app, github_push(&evil, Some(&sig), "d4")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn redelivery_is_acknowledged_once() {
        let app = app().await;
        let sig = sign_hmac_sha256(PUSH.as_bytes(), SECRET.as_bytes());

        let (s1, b1) = send(&app, github_push(PUSH, Some(&sig), "same-id")).await;
        assert_eq!(s1, StatusCode::ACCEPTED, "{b1}");

        // Byte-identical, correctly signed, same delivery id. Only the guard can
        // tell it is a repeat.
        let (s2, b2) = send(&app, github_push(PUSH, Some(&sig), "same-id")).await;
        assert_eq!(s2, StatusCode::OK, "{b2}");
        assert!(b2.contains("duplicate"), "{b2}");
    }

    #[tokio::test]
    async fn ping_is_acknowledged_without_a_run() {
        let app = app().await;
        let body = r#"{"zen":"Keep it simple.","repository":{"full_name":"acme/web"}}"#;
        let sig = sign_hmac_sha256(body.as_bytes(), SECRET.as_bytes());
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/webhooks/github")
            .header("X-GitHub-Event", "ping")
            .header("X-GitHub-Delivery", "ping-1")
            .header("X-Hub-Signature-256", sig)
            .body(axum::body::Body::from(body))
            .unwrap();
        let (status, b) = send(&app, req).await;
        assert_eq!(status, StatusCode::OK, "{b}");
        assert!(b.contains("ignored"), "{b}");
    }

    #[tokio::test]
    async fn uninteresting_events_return_success_not_error() {
        // Providers disable endpoints that keep failing, so an ignored event must
        // not look like a failure.
        let app = app().await;
        let body = r#"{"repository":{"full_name":"acme/web"}}"#;
        let sig = sign_hmac_sha256(body.as_bytes(), SECRET.as_bytes());
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/webhooks/github")
            .header("X-GitHub-Event", "star")
            .header("X-GitHub-Delivery", "star-1")
            .header("X-Hub-Signature-256", sig)
            .body(axum::body::Body::from(body))
            .unwrap();
        let (status, b) = send(&app, req).await;
        assert_eq!(status, StatusCode::OK, "{b}");
    }

    #[tokio::test]
    async fn gitlab_token_path_works() {
        let app = app().await;
        let body = r#"{"ref":"refs/heads/main","checkout_sha":"1","project":{"path_with_namespace":"acme/web"}}"#;
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/webhooks/gitlab")
            .header("X-Gitlab-Event", "Push Hook")
            .header("X-Gitlab-Token", SECRET)
            .body(axum::body::Body::from(body))
            .unwrap();
        let (status, b) = send(&app, req).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{b}");
    }

    #[tokio::test]
    async fn gitlab_wrong_token_is_rejected() {
        let app = app().await;
        let body = r#"{"ref":"refs/heads/main","project":{"path_with_namespace":"acme/web"}}"#;
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/webhooks/gitlab")
            .header("X-Gitlab-Event", "Push Hook")
            .header("X-Gitlab-Token", "nope")
            .body(axum::body::Body::from(body))
            .unwrap();
        let (status, _) = send(&app, req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn bitbucket_signed_push_works() {
        let app = app().await;
        let body = r#"{"push":{"changes":[{"new":{"type":"branch","name":"main","target":{"hash":"h"}}}]},"repository":{"full_name":"acme/web"}}"#;
        let sig = sign_hmac_sha256(body.as_bytes(), SECRET.as_bytes());
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/webhooks/bitbucket")
            .header("X-Event-Key", "repo:push")
            .header("X-Request-UUID", "bb-1")
            .header("X-Hub-Signature", sig)
            .body(axum::body::Body::from(body))
            .unwrap();
        let (status, b) = send(&app, req).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{b}");
    }

    #[tokio::test]
    async fn github_sha1_signature_is_refused() {
        // The downgrade an attacker would choose: GitHub's legacy header is SHA-1
        // and shares its name with Bitbucket's SHA-256 header.
        let app = app().await;
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/webhooks/github")
            .header("X-GitHub-Event", "push")
            .header("X-Hub-Signature", "sha1=0000000000000000000000000000000000000000")
            .body(axum::body::Body::from(PUSH))
            .unwrap();
        let (status, _) = send(&app, req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn unknown_provider_is_404() {
        let app = app().await;
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/webhooks/perforce")
            .body(axum::body::Body::from("{}"))
            .unwrap();
        let (status, _) = send(&app, req).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn oversized_body_is_refused_before_hmac() {
        // The layer must bite before any crypto runs.
        let app = app().await;
        let huge = "x".repeat(MAX_BODY_BYTES + 1024);
        let sig = sign_hmac_sha256(huge.as_bytes(), SECRET.as_bytes());
        let (status, _) = send(&app, github_push(&huge, Some(&sig), "big")).await;
        assert_eq!(
            status,
            StatusCode::PAYLOAD_TOO_LARGE,
            "body cap must apply before the signature check"
        );
    }

    #[tokio::test]
    async fn api_trigger_requires_a_bearer_token() {
        let app = app().await;
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/trigger")
            .body(axum::body::Body::from(r#"{"project":"web","pipeline":"ci"}"#))
            .unwrap();
        let (status, _) = send(&app, req).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_trigger_rejects_an_absurd_token_before_hashing() {
        let app = app().await;
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/trigger")
            .header("Authorization", format!("Bearer {}", "x".repeat(4096)))
            .body(axum::body::Body::from(r#"{"project":"web","pipeline":"ci"}"#))
            .unwrap();
        let (status, _) = send(&app, req).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
