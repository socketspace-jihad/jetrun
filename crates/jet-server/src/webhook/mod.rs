//! Inbound webhooks.
//!
//! # Order of operations is security-critical
//!
//! 1. read the body as **raw bytes**, under a size cap;
//! 2. verify the signature over those exact bytes ([`verify`]);
//! 3. reject replays ([`dedupe`]);
//! 4. only then parse JSON and normalize.
//!
//! Step 2 must precede step 4. Parsing and re-serializing produces semantically
//! identical, cryptographically different bytes, so verifying a reparsed body can
//! never work -- and "verify later" invariably becomes "verify never". The size
//! cap precedes everything because HMAC over an unbounded body is itself a DoS.
//!
//! # A note on what these webhooks are
//!
//! The events here come from the **version control host** -- GitHub, GitLab,
//! Bitbucket -- not from GitHub Actions, GitLab CI, or Bitbucket Pipelines, which
//! are the CI products jetrun replaces. Triggering jetrun *from* an existing
//! pipeline (a GitHub Actions job calling out to us during migration) is a
//! different flow: it authenticates with a jetrun API token and is handled by the
//! generic trigger endpoint, not by signature verification. Both paths are
//! supported, and conflating them would mean either trusting an unsigned request
//! or asking GitHub to sign something it never sends.

pub mod dedupe;
pub mod verify;

use std::fmt;

use serde_json::Value;

pub use dedupe::{Delivery, DeliveryGuard};
pub use verify::{HeaderLookup, Verified, VerifyError, sign_hmac_sha256};

/// Largest webhook body accepted.
///
/// GitHub caps deliveries around 25 MiB, but a monorepo push event is a few
/// hundred kilobytes at most and we only read a handful of fields. Capping at
/// 2 MiB bounds the HMAC cost per request and rejects absurd payloads before any
/// crypto runs.
pub const MAX_BODY_BYTES: usize = 2 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    GitHub,
    GitLab,
    Bitbucket,
}

impl Provider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Provider::GitHub => "github",
            Provider::GitLab => "gitlab",
            Provider::Bitbucket => "bitbucket",
        }
    }

    /// Header carrying the event name.
    pub const fn event_header(self) -> &'static str {
        match self {
            Provider::GitHub => "x-github-event",
            Provider::GitLab => "x-gitlab-event",
            Provider::Bitbucket => "x-event-key",
        }
    }

    /// Header carrying the unique delivery id, used for replay rejection.
    pub const fn delivery_header(self) -> &'static str {
        match self {
            Provider::GitHub => "x-github-delivery",
            // GitLab only sends this on some event types; absence is tolerated.
            Provider::GitLab => "x-gitlab-event-uuid",
            Provider::Bitbucket => "x-request-uuid",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What happened, in jetrun's own vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    Push {
        branch: String,
    },
    Tag {
        name: String,
    },
    PullRequest {
        number: u64,
        action: String,
        source_branch: String,
        target_branch: Option<String>,
    },
    /// A provider health check (GitHub `ping`, GitLab test delivery). Must be
    /// answered successfully but must not start a run -- otherwise clicking "Test
    /// webhook" in the UI burns a build.
    Ping,
    /// Recognized delivery for an event we do not act on. Acknowledged and
    /// ignored, which is different from an error: providers disable endpoints
    /// that return failures.
    Ignored {
        event: String,
    },
}

/// A normalized trigger, provider-independent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerEvent {
    pub provider: Provider,
    /// Provider-assigned delivery id, when present.
    pub delivery_id: Option<String>,
    pub kind: EventKind,
    /// `owner/repo`, `group/subgroup/project`, or `workspace/repo`.
    pub repo: String,
    pub commit_sha: Option<String>,
    /// Full git ref where the provider supplies one.
    pub reference: Option<String>,
    pub actor: Option<String>,
    /// Whether the payload bytes were cryptographically authenticated (false for
    /// GitLab, which only proves the sender knew a shared secret).
    pub body_authenticated: bool,
}

impl TriggerEvent {
    /// Whether this event should start a run.
    pub fn should_trigger(&self) -> bool {
        match &self.kind {
            EventKind::Push { .. } | EventKind::Tag { .. } => true,
            EventKind::PullRequest { action, .. } => matches!(
                action.as_str(),
                // Normalized in the per-provider parsers below.
                "opened" | "reopened" | "synchronize" | "updated"
            ),
            EventKind::Ping | EventKind::Ignored { .. } => false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    #[error(transparent)]
    Verify(#[from] VerifyError),
    #[error("body exceeds {MAX_BODY_BYTES} bytes")]
    BodyTooLarge,
    #[error("missing {0} header")]
    MissingHeader(&'static str),
    #[error("payload is not valid JSON: {0}")]
    BadJson(String),
    #[error("payload is missing required field {0}")]
    MissingField(&'static str),
    #[error("replayed delivery {0}")]
    Replay(String),
}

/// Parse a verified payload into a normalized event.
///
/// Called only after [`verify::verify`] has succeeded.
pub fn parse(
    provider: Provider,
    headers: &dyn HeaderLookup,
    body: &[u8],
    verified: Verified,
) -> Result<TriggerEvent, WebhookError> {
    let event = headers
        .get(provider.event_header())
        .ok_or(WebhookError::MissingHeader(provider.event_header()))?
        .to_owned();
    let delivery_id = headers.get(provider.delivery_header()).map(str::to_owned);

    let json: Value =
        serde_json::from_slice(body).map_err(|e| WebhookError::BadJson(e.to_string()))?;

    let (kind, repo, commit_sha, reference, actor) = match provider {
        Provider::GitHub => github(&event, &json)?,
        Provider::GitLab => gitlab(&event, &json)?,
        Provider::Bitbucket => bitbucket(&event, &json)?,
    };

    Ok(TriggerEvent {
        provider,
        delivery_id,
        kind,
        repo,
        commit_sha,
        reference,
        actor,
        body_authenticated: verified.body_authenticated,
    })
}

type Parsed = (
    EventKind,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn s(v: &Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for p in path {
        cur = cur.get(p)?;
    }
    cur.as_str().map(str::to_owned)
}

fn u(v: &Value, path: &[&str]) -> Option<u64> {
    let mut cur = v;
    for p in path {
        cur = cur.get(p)?;
    }
    cur.as_u64()
}

/// Strip `refs/heads/` or `refs/tags/`.
fn short_ref(r: &str) -> String {
    r.strip_prefix("refs/heads/")
        .or_else(|| r.strip_prefix("refs/tags/"))
        .unwrap_or(r)
        .to_owned()
}

fn github(event: &str, j: &Value) -> Result<Parsed, WebhookError> {
    let repo = s(j, &["repository", "full_name"]).unwrap_or_default();

    match event {
        "ping" => Ok((EventKind::Ping, repo, None, None, None)),
        "push" => {
            let reference = s(j, &["ref"]).ok_or(WebhookError::MissingField("ref"))?;
            // GitHub reports branch and tag pushes through the same event; the ref
            // prefix is the only discriminator.
            let kind = if reference.starts_with("refs/tags/") {
                EventKind::Tag {
                    name: short_ref(&reference),
                }
            } else {
                EventKind::Push {
                    branch: short_ref(&reference),
                }
            };
            Ok((
                kind,
                repo,
                s(j, &["after"]),
                Some(reference),
                s(j, &["pusher", "name"]),
            ))
        }
        "pull_request" => {
            let action = s(j, &["action"]).unwrap_or_default();
            Ok((
                EventKind::PullRequest {
                    number: u(j, &["number"])
                        .or_else(|| u(j, &["pull_request", "number"]))
                        .unwrap_or(0),
                    action,
                    source_branch: s(j, &["pull_request", "head", "ref"]).unwrap_or_default(),
                    target_branch: s(j, &["pull_request", "base", "ref"]),
                },
                repo,
                s(j, &["pull_request", "head", "sha"]),
                None,
                s(j, &["sender", "login"]),
            ))
        }
        other => Ok((
            EventKind::Ignored {
                event: other.to_owned(),
            },
            repo,
            None,
            None,
            None,
        )),
    }
}

fn gitlab(event: &str, j: &Value) -> Result<Parsed, WebhookError> {
    let repo = s(j, &["project", "path_with_namespace"]).unwrap_or_default();
    let actor = s(j, &["user_username"]).or_else(|| s(j, &["user", "username"]));

    match event {
        // GitLab's "Test" button sends a Push Hook with a synthetic payload; there
        // is no dedicated ping event, so nothing to special-case here.
        "Push Hook" => {
            let reference = s(j, &["ref"]).ok_or(WebhookError::MissingField("ref"))?;
            Ok((
                EventKind::Push {
                    branch: short_ref(&reference),
                },
                repo,
                s(j, &["checkout_sha"]).or_else(|| s(j, &["after"])),
                Some(reference),
                actor,
            ))
        }
        "Tag Push Hook" => {
            let reference = s(j, &["ref"]).ok_or(WebhookError::MissingField("ref"))?;
            Ok((
                EventKind::Tag {
                    name: short_ref(&reference),
                },
                repo,
                s(j, &["checkout_sha"]).or_else(|| s(j, &["after"])),
                Some(reference),
                actor,
            ))
        }
        "Merge Request Hook" => {
            // GitLab's action vocabulary differs from GitHub's; normalize so
            // `should_trigger` has one set of names to reason about.
            let raw = s(j, &["object_attributes", "action"]).unwrap_or_default();
            let action = match raw.as_str() {
                "open" => "opened",
                "reopen" => "reopened",
                "update" => "synchronize",
                other => other,
            }
            .to_owned();

            Ok((
                EventKind::PullRequest {
                    number: u(j, &["object_attributes", "iid"]).unwrap_or(0),
                    action,
                    source_branch: s(j, &["object_attributes", "source_branch"])
                        .unwrap_or_default(),
                    target_branch: s(j, &["object_attributes", "target_branch"]),
                },
                repo,
                s(j, &["object_attributes", "last_commit", "id"]),
                None,
                actor,
            ))
        }
        other => Ok((
            EventKind::Ignored {
                event: other.to_owned(),
            },
            repo,
            None,
            None,
            None,
        )),
    }
}

fn bitbucket(event: &str, j: &Value) -> Result<Parsed, WebhookError> {
    let repo = s(j, &["repository", "full_name"]).unwrap_or_default();
    let actor = s(j, &["actor", "nickname"]).or_else(|| s(j, &["actor", "display_name"]));

    match event {
        "diagnostics:ping" => Ok((EventKind::Ping, repo, None, None, None)),
        "repo:push" => {
            // Bitbucket batches changes into an array, and `new` is null when a
            // branch was deleted -- a deletion must not be read as a push to a
            // branch named "".
            let change = j
                .get("push")
                .and_then(|p| p.get("changes"))
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .ok_or(WebhookError::MissingField("push.changes"))?;

            let new = match change.get("new") {
                Some(Value::Null) | None => {
                    return Ok((
                        EventKind::Ignored {
                            event: "repo:push (branch deleted)".into(),
                        },
                        repo,
                        None,
                        None,
                        actor,
                    ));
                }
                Some(v) => v,
            };

            let name = s(new, &["name"]).unwrap_or_default();
            let sha = s(new, &["target", "hash"]);
            let kind = match s(new, &["type"]).as_deref() {
                Some("tag") => EventKind::Tag { name: name.clone() },
                _ => EventKind::Push {
                    branch: name.clone(),
                },
            };
            let reference = match &kind {
                EventKind::Tag { .. } => Some(format!("refs/tags/{name}")),
                _ => Some(format!("refs/heads/{name}")),
            };
            Ok((kind, repo, sha, reference, actor))
        }
        e if e.starts_with("pullrequest:") => {
            let action = match e {
                "pullrequest:created" => "opened",
                "pullrequest:updated" => "synchronize",
                other => other.trim_start_matches("pullrequest:"),
            }
            .to_owned();
            Ok((
                EventKind::PullRequest {
                    number: u(j, &["pullrequest", "id"]).unwrap_or(0),
                    action,
                    source_branch: s(j, &["pullrequest", "source", "branch", "name"])
                        .unwrap_or_default(),
                    target_branch: s(j, &["pullrequest", "destination", "branch", "name"]),
                },
                repo,
                s(j, &["pullrequest", "source", "commit", "hash"]),
                None,
                actor,
            ))
        }
        other => Ok((
            EventKind::Ignored {
                event: other.to_owned(),
            },
            repo,
            None,
            None,
            None,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn hdrs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn ok() -> Verified {
        Verified {
            body_authenticated: true,
        }
    }

    // ------------------------------------------------------------- GitHub

    #[test]
    fn github_push_normalizes() {
        let body = br#"{
            "ref": "refs/heads/main",
            "after": "aaaabbbbccccdddd",
            "repository": {"full_name": "acme/web"},
            "pusher": {"name": "dev"}
        }"#;
        let h = hdrs(&[("X-GitHub-Event", "push"), ("X-GitHub-Delivery", "d-1")]);
        let ev = parse(Provider::GitHub, &h, body, ok()).unwrap();

        assert_eq!(
            ev.kind,
            EventKind::Push {
                branch: "main".into()
            }
        );
        assert_eq!(ev.repo, "acme/web");
        assert_eq!(ev.commit_sha.as_deref(), Some("aaaabbbbccccdddd"));
        assert_eq!(ev.reference.as_deref(), Some("refs/heads/main"));
        assert_eq!(ev.actor.as_deref(), Some("dev"));
        assert_eq!(ev.delivery_id.as_deref(), Some("d-1"));
        assert!(ev.should_trigger());
    }

    #[test]
    fn github_tag_push_is_a_tag_not_a_branch() {
        // GitHub sends tags through the `push` event; only the ref prefix tells
        // them apart, and treating a tag as a branch named "refs/tags/v1" would
        // silently run the wrong pipeline.
        let body = br#"{"ref":"refs/tags/v1.2.3","after":"sha","repository":{"full_name":"acme/web"}}"#;
        let h = hdrs(&[("X-GitHub-Event", "push")]);
        let ev = parse(Provider::GitHub, &h, body, ok()).unwrap();
        assert_eq!(
            ev.kind,
            EventKind::Tag {
                name: "v1.2.3".into()
            }
        );
    }

    #[test]
    fn github_ping_is_acknowledged_but_does_not_trigger() {
        // Clicking "Test webhook" must not burn a build.
        let h = hdrs(&[("X-GitHub-Event", "ping")]);
        let ev = parse(Provider::GitHub, &h, br#"{"zen":"hi"}"#, ok()).unwrap();
        assert_eq!(ev.kind, EventKind::Ping);
        assert!(!ev.should_trigger());
    }

    #[test]
    fn github_pull_request_actions_are_filtered() {
        for (action, expect) in [
            ("opened", true),
            ("reopened", true),
            ("synchronize", true),
            ("closed", false),
            ("labeled", false),
            ("assigned", false),
        ] {
            let body = format!(
                r#"{{"action":"{action}","number":7,
                     "pull_request":{{"head":{{"ref":"feat","sha":"s"}},"base":{{"ref":"main"}}}},
                     "repository":{{"full_name":"acme/web"}}}}"#
            );
            let h = hdrs(&[("X-GitHub-Event", "pull_request")]);
            let ev = parse(Provider::GitHub, &h, body.as_bytes(), ok()).unwrap();
            assert_eq!(ev.should_trigger(), expect, "action {action}");
            if expect {
                assert_eq!(
                    ev.kind,
                    EventKind::PullRequest {
                        number: 7,
                        action: action.into(),
                        source_branch: "feat".into(),
                        target_branch: Some("main".into()),
                    }
                );
            }
        }
    }

    #[test]
    fn github_unknown_event_is_ignored_not_an_error() {
        // Returning an error would make GitHub disable the endpoint after enough
        // failures, taking out every pipeline.
        let h = hdrs(&[("X-GitHub-Event", "star")]);
        let ev = parse(Provider::GitHub, &h, br#"{"repository":{"full_name":"a/b"}}"#, ok()).unwrap();
        assert_eq!(
            ev.kind,
            EventKind::Ignored {
                event: "star".into()
            }
        );
        assert!(!ev.should_trigger());
    }

    // ------------------------------------------------------------- GitLab

    #[test]
    fn gitlab_push_normalizes() {
        let body = br#"{
            "ref": "refs/heads/develop",
            "checkout_sha": "1111",
            "project": {"path_with_namespace": "group/sub/app"},
            "user_username": "alice"
        }"#;
        let h = hdrs(&[("X-Gitlab-Event", "Push Hook")]);
        let ev = parse(Provider::GitLab, &h, body, ok()).unwrap();
        assert_eq!(
            ev.kind,
            EventKind::Push {
                branch: "develop".into()
            }
        );
        assert_eq!(ev.repo, "group/sub/app", "nested groups must survive");
        assert_eq!(ev.commit_sha.as_deref(), Some("1111"));
        assert_eq!(ev.actor.as_deref(), Some("alice"));
    }

    #[test]
    fn gitlab_merge_request_actions_are_normalized_to_common_names() {
        // GitLab says "open"/"reopen"/"update" where GitHub says
        // "opened"/"reopened"/"synchronize". Normalizing here means the trigger
        // rules have one vocabulary instead of three.
        for (gitlab_action, normalized) in [
            ("open", "opened"),
            ("reopen", "reopened"),
            ("update", "synchronize"),
        ] {
            let body = format!(
                r#"{{"object_attributes":{{"iid":42,"action":"{gitlab_action}",
                     "source_branch":"feat","target_branch":"main",
                     "last_commit":{{"id":"abc"}}}},
                     "project":{{"path_with_namespace":"g/p"}}}}"#
            );
            let h = hdrs(&[("X-Gitlab-Event", "Merge Request Hook")]);
            let ev = parse(Provider::GitLab, &h, body.as_bytes(), ok()).unwrap();
            assert_eq!(
                ev.kind,
                EventKind::PullRequest {
                    number: 42,
                    action: normalized.into(),
                    source_branch: "feat".into(),
                    target_branch: Some("main".into()),
                }
            );
            assert!(ev.should_trigger(), "{gitlab_action} should trigger");
        }
    }

    #[test]
    fn gitlab_merge_request_close_does_not_trigger() {
        let body = br#"{"object_attributes":{"iid":1,"action":"close","source_branch":"f"},
                        "project":{"path_with_namespace":"g/p"}}"#;
        let h = hdrs(&[("X-Gitlab-Event", "Merge Request Hook")]);
        assert!(!parse(Provider::GitLab, &h, body, ok()).unwrap().should_trigger());
    }

    #[test]
    fn gitlab_tag_hook_is_a_tag() {
        let body = br#"{"ref":"refs/tags/v2","checkout_sha":"t","project":{"path_with_namespace":"g/p"}}"#;
        let h = hdrs(&[("X-Gitlab-Event", "Tag Push Hook")]);
        assert_eq!(
            parse(Provider::GitLab, &h, body, ok()).unwrap().kind,
            EventKind::Tag { name: "v2".into() }
        );
    }

    #[test]
    fn gitlab_events_report_that_the_body_is_not_authenticated() {
        let body = br#"{"ref":"refs/heads/main","project":{"path_with_namespace":"g/p"}}"#;
        let h = hdrs(&[("X-Gitlab-Event", "Push Hook")]);
        let ev = parse(
            Provider::GitLab,
            &h,
            body,
            Verified {
                body_authenticated: false,
            },
        )
        .unwrap();
        assert!(
            !ev.body_authenticated,
            "downstream policy may want to treat GitLab payloads with less trust"
        );
    }

    // ---------------------------------------------------------- Bitbucket

    #[test]
    fn bitbucket_push_normalizes() {
        let body = br#"{
            "push": {"changes": [{"new": {"type":"branch","name":"main",
                     "target": {"hash": "cafe"}}}]},
            "repository": {"full_name": "team/repo"},
            "actor": {"nickname": "bob"}
        }"#;
        let h = hdrs(&[("X-Event-Key", "repo:push"), ("X-Request-UUID", "u-9")]);
        let ev = parse(Provider::Bitbucket, &h, body, ok()).unwrap();
        assert_eq!(
            ev.kind,
            EventKind::Push {
                branch: "main".into()
            }
        );
        assert_eq!(ev.repo, "team/repo");
        assert_eq!(ev.commit_sha.as_deref(), Some("cafe"));
        assert_eq!(ev.reference.as_deref(), Some("refs/heads/main"));
        assert_eq!(ev.delivery_id.as_deref(), Some("u-9"));
    }

    #[test]
    fn bitbucket_branch_deletion_is_not_a_push() {
        // `new` is null on deletion. Reading it naively yields a push to a branch
        // named "", which would trigger a nonsense run.
        let body = br#"{
            "push": {"changes": [{"new": null, "old": {"name":"gone"}}]},
            "repository": {"full_name": "team/repo"}
        }"#;
        let h = hdrs(&[("X-Event-Key", "repo:push")]);
        let ev = parse(Provider::Bitbucket, &h, body, ok()).unwrap();
        assert!(!ev.should_trigger());
        assert!(matches!(ev.kind, EventKind::Ignored { .. }));
    }

    #[test]
    fn bitbucket_tag_push_is_a_tag() {
        let body = br#"{
            "push": {"changes": [{"new": {"type":"tag","name":"v3",
                     "target": {"hash": "t3"}}}]},
            "repository": {"full_name": "team/repo"}
        }"#;
        let h = hdrs(&[("X-Event-Key", "repo:push")]);
        let ev = parse(Provider::Bitbucket, &h, body, ok()).unwrap();
        assert_eq!(ev.kind, EventKind::Tag { name: "v3".into() });
        assert_eq!(ev.reference.as_deref(), Some("refs/tags/v3"));
    }

    #[test]
    fn bitbucket_pullrequest_events_normalize() {
        let body = br#"{
            "pullrequest": {"id": 12,
              "source": {"branch": {"name":"feat"}, "commit": {"hash":"s1"}},
              "destination": {"branch": {"name":"main"}}},
            "repository": {"full_name": "team/repo"}
        }"#;
        let h = hdrs(&[("X-Event-Key", "pullrequest:created")]);
        let ev = parse(Provider::Bitbucket, &h, body, ok()).unwrap();
        assert_eq!(
            ev.kind,
            EventKind::PullRequest {
                number: 12,
                action: "opened".into(),
                source_branch: "feat".into(),
                target_branch: Some("main".into()),
            }
        );
        assert!(ev.should_trigger());
    }

    #[test]
    fn bitbucket_pullrequest_rejected_does_not_trigger() {
        let body = br#"{"pullrequest":{"id":1,"source":{"branch":{"name":"f"}}},
                        "repository":{"full_name":"t/r"}}"#;
        let h = hdrs(&[("X-Event-Key", "pullrequest:rejected")]);
        assert!(
            !parse(Provider::Bitbucket, &h, body, ok())
                .unwrap()
                .should_trigger()
        );
    }

    // --------------------------------------------------------- robustness

    #[test]
    fn missing_event_header_is_an_error() {
        let h = hdrs(&[("X-GitHub-Delivery", "d")]);
        assert!(matches!(
            parse(Provider::GitHub, &h, b"{}", ok()),
            Err(WebhookError::MissingHeader(_))
        ));
    }

    #[test]
    fn malformed_json_is_rejected() {
        let h = hdrs(&[("X-GitHub-Event", "push")]);
        assert!(matches!(
            parse(Provider::GitHub, &h, b"{not json", ok()),
            Err(WebhookError::BadJson(_))
        ));
    }

    #[test]
    fn missing_required_field_is_reported() {
        let h = hdrs(&[("X-GitHub-Event", "push")]);
        assert!(matches!(
            parse(Provider::GitHub, &h, br#"{"repository":{"full_name":"a/b"}}"#, ok()),
            Err(WebhookError::MissingField("ref"))
        ));
    }

    #[test]
    fn unexpected_field_types_do_not_panic() {
        // Real payloads drift and mock payloads are wrong; a type mismatch must
        // degrade, never panic.
        let h = hdrs(&[("X-GitHub-Event", "pull_request")]);
        let body = br#"{"action":123,"number":"seven",
                        "pull_request":{"head":[]},"repository":{"full_name":null}}"#;
        let ev = parse(Provider::GitHub, &h, body, ok()).unwrap();
        assert_eq!(ev.repo, "");
        assert!(matches!(ev.kind, EventKind::PullRequest { number: 0, .. }));
    }

    #[test]
    fn deeply_nested_json_does_not_blow_the_stack() {
        // serde_json has a recursion limit; confirm we surface it as an error.
        let deep = format!("{}{}", "[".repeat(2000), "]".repeat(2000));
        let h = hdrs(&[("X-GitHub-Event", "push")]);
        let got = parse(Provider::GitHub, &h, deep.as_bytes(), ok());
        assert!(got.is_err(), "deeply nested input must be refused");
    }

    #[test]
    fn refs_with_slashes_keep_their_full_name() {
        let body = br#"{"ref":"refs/heads/feature/JET-42/fix","after":"s",
                        "repository":{"full_name":"a/b"}}"#;
        let h = hdrs(&[("X-GitHub-Event", "push")]);
        let ev = parse(Provider::GitHub, &h, body, ok()).unwrap();
        assert_eq!(
            ev.kind,
            EventKind::Push {
                branch: "feature/JET-42/fix".into()
            }
        );
    }
}
