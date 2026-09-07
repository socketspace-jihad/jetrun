use serde::{Deserialize, Serialize};

/// Normalized webhook event — the common representation regardless of provider.
/// Each provider's parser converts its raw payload into this struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEvent {
    pub provider: WebhookProvider,
    pub event_type: WebhookEventType,
    pub repo_url: String,
    pub repo_name: String,
    pub branch: String,
    pub commit_sha: String,
    pub commit_message: Option<String>,
    pub author: Option<String>,
    /// For PRs: source branch
    pub source_branch: Option<String>,
    /// For PRs: target branch
    pub target_branch: Option<String>,
    /// For PRs: PR number
    pub pr_number: Option<u64>,
    /// For PRs: PR title
    pub pr_title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookProvider {
    Github,
    Gitlab,
    Bitbucket,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEventType {
    Push,
    PullRequest,
    Tag,
    Unknown,
}

impl std::fmt::Display for WebhookProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Github => write!(f, "github"),
            Self::Gitlab => write!(f, "gitlab"),
            Self::Bitbucket => write!(f, "bitbucket"),
            Self::Generic => write!(f, "generic"),
        }
    }
}
