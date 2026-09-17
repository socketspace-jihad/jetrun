use std::path::Path;
use tokio::process::Command;

/// Clone a repo with depth 1 (shallow clone for speed)
pub async fn clone(repo_url: &str, branch: &str, dest: &Path) -> anyhow::Result<()> {
    let output = Command::new("git")
        .args([
            "clone",
            "--depth", "1",
            "--branch", branch,
            repo_url,
            dest.to_str().unwrap_or(""),
        ])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git clone failed: {}", stderr.trim());
    }

    tracing::debug!(repo = %repo_url, branch = %branch, "cloned repo");
    Ok(())
}

/// Pull latest changes in an existing repo
pub async fn pull(repo_path: &Path, branch: &str) -> anyhow::Result<()> {
    // Fetch
    let output = Command::new("git")
        .args(["fetch", "origin", branch])
        .current_dir(repo_path)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git fetch failed: {}", stderr.trim());
    }

    // Reset to origin/branch
    let output = Command::new("git")
        .args(["reset", "--hard", &format!("origin/{}", branch)])
        .current_dir(repo_path)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git reset failed: {}", stderr.trim());
    }

    tracing::debug!(path = ?repo_path, branch = %branch, "pulled latest");
    Ok(())
}

/// Get the current commit SHA
pub async fn current_sha(repo_path: &Path) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .await?;

    if !output.status.success() {
        anyhow::bail!("git rev-parse failed");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
