//! Artifact collection — copy build outputs from workspace to persistent storage.
//! Layout: {data_dir}/artifacts/{build_id}/{artifact_name}/{file_path}

use std::path::{Path, PathBuf};
use uuid::Uuid;

use jetrun_broker::types::ArtifactInfo;

/// Collect artifacts from workspace after a stage succeeds.
/// Copies files matching `artifact.paths` from workspace to artifact storage.
pub async fn collect(
    data_dir: &Path,
    build_id: Uuid,
    workspace: &Path,
    artifact: &ArtifactInfo,
) -> anyhow::Result<Vec<String>> {
    let dest_dir = artifact_dir(data_dir, build_id, &artifact.name);
    tokio::fs::create_dir_all(&dest_dir).await?;

    let mut collected = Vec::new();

    for pattern in &artifact.paths {
        let src = workspace.join(pattern);

        if src.is_file() {
            // Single file
            let file_name = src.file_name().unwrap_or_default();
            let dest = dest_dir.join(file_name);
            if let Err(e) = tokio::fs::copy(&src, &dest).await {
                tracing::warn!(path = %pattern, error = %e, "artifact copy failed");
            } else {
                collected.push(file_name.to_string_lossy().to_string());
                tracing::info!(artifact = %artifact.name, file = %file_name.to_string_lossy(), "artifact collected");
            }
        } else if src.is_dir() {
            // Directory — copy recursively
            if let Err(e) = copy_dir_recursive(&src, &dest_dir).await {
                tracing::warn!(path = %pattern, error = %e, "artifact dir copy failed");
            } else {
                collected.push(pattern.clone());
            }
        } else {
            // Try glob
            let full_pattern = workspace.join(pattern).to_string_lossy().to_string();
            if let Ok(entries) = glob::glob(&full_pattern) {
                for entry in entries.flatten() {
                    if entry.is_file() {
                        let relative = entry.strip_prefix(workspace).unwrap_or(&entry);
                        let dest = dest_dir.join(relative);
                        if let Some(parent) = dest.parent() {
                            let _ = tokio::fs::create_dir_all(parent).await;
                        }
                        if tokio::fs::copy(&entry, &dest).await.is_ok() {
                            collected.push(relative.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }

    tracing::info!(
        build_id = %build_id,
        artifact = %artifact.name,
        files = collected.len(),
        "artifacts collected"
    );

    Ok(collected)
}

/// List artifacts for a build
pub async fn list_artifacts(data_dir: &Path, build_id: Uuid) -> Vec<ArtifactEntry> {
    let base = data_dir.join("artifacts").join(build_id.to_string());
    let mut entries = Vec::new();

    let mut dir = match tokio::fs::read_dir(&base).await {
        Ok(d) => d,
        Err(_) => return entries,
    };

    while let Ok(Some(entry)) = dir.next_entry().await {
        if !entry.path().is_dir() { continue; }
        let name = entry.file_name().to_string_lossy().to_string();

        // List files in this artifact
        let mut files = Vec::new();
        if let Ok(size) = dir_size(&entry.path()).await {
            files_recursive(&entry.path(), &entry.path(), &mut files).await;
            entries.push(ArtifactEntry { name, files, total_bytes: size });
        }
    }

    entries
}

#[derive(Debug, serde::Serialize)]
pub struct ArtifactEntry {
    pub name: String,
    pub files: Vec<String>,
    pub total_bytes: u64,
}

/// Get path to a specific artifact file for download
pub fn artifact_file_path(data_dir: &Path, build_id: Uuid, artifact_name: &str, file_path: &str) -> PathBuf {
    data_dir.join("artifacts").join(build_id.to_string()).join(artifact_name).join(file_path)
}

fn artifact_dir(data_dir: &Path, build_id: Uuid, artifact_name: &str) -> PathBuf {
    data_dir.join("artifacts").join(build_id.to_string()).join(artifact_name)
}

async fn copy_dir_recursive(src: &Path, dest: &Path) -> anyhow::Result<()> {
    let mut dir = tokio::fs::read_dir(src).await?;
    while let Some(entry) = dir.next_entry().await? {
        let path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if path.is_dir() {
            tokio::fs::create_dir_all(&dest_path).await?;
            Box::pin(copy_dir_recursive(&path, &dest_path)).await?;
        } else {
            tokio::fs::copy(&path, &dest_path).await?;
        }
    }
    Ok(())
}

async fn dir_size(path: &Path) -> anyhow::Result<u64> {
    let mut total = 0u64;
    let mut dir = tokio::fs::read_dir(path).await?;
    while let Some(entry) = dir.next_entry().await? {
        let p = entry.path();
        if p.is_file() {
            total += tokio::fs::metadata(&p).await.map(|m| m.len()).unwrap_or(0);
        } else if p.is_dir() {
            total += Box::pin(dir_size(&p)).await.unwrap_or(0);
        }
    }
    Ok(total)
}

async fn files_recursive(base: &Path, current: &Path, out: &mut Vec<String>) {
    let mut dir = match tokio::fs::read_dir(current).await {
        Ok(d) => d,
        Err(_) => return,
    };
    while let Ok(Some(entry)) = dir.next_entry().await {
        let p = entry.path();
        if p.is_file() {
            let rel = p.strip_prefix(base).unwrap_or(&p);
            out.push(rel.to_string_lossy().to_string());
        } else if p.is_dir() {
            Box::pin(files_recursive(base, &p, out)).await;
        }
    }
}
