use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use blake3::Hasher;

use jetrun_common::models::{StepConfig, StepFingerprint};

/// Compute a deterministic fingerprint for a build step.
///
/// The fingerprint is: `blake3(command || sorted_env || sorted_input_file_hashes)`
///
/// If the command, environment variables, and all input files are identical
/// to a previous run, the step will produce the same output and can be skipped.
/// This is the core mechanism for content-addressable build skipping.
pub async fn compute_fingerprint(
    step: &StepConfig,
    working_dir: &Path,
    input_patterns: &[String],
) -> anyhow::Result<StepFingerprint> {
    let mut hasher = Hasher::new();

    // 1. Hash the command
    hasher.update(step.run.as_bytes());
    hasher.update(b"\x00");

    // 2. Hash the Docker image (if any)
    if let Some(image) = &step.image {
        hasher.update(b"image:");
        hasher.update(image.as_bytes());
        hasher.update(b"\x00");
    }

    // 3. Hash environment variables (sorted for determinism)
    let sorted_env: BTreeMap<_, _> = step.env.iter().collect();
    for (key, value) in &sorted_env {
        hasher.update(key.as_bytes());
        hasher.update(b"=");
        hasher.update(value.as_bytes());
        hasher.update(b"\x00");
    }

    // 4. Hash input files
    let file_hashes = if input_patterns.is_empty() {
        // No patterns specified — hash all files in working_dir (limited depth)
        let wd = working_dir.to_owned();
        tokio::task::spawn_blocking(move || walk_and_hash(&wd, 3)).await??
    } else {
        // Hash files matching glob patterns
        let wd = working_dir.to_owned();
        let patterns: Vec<String> = input_patterns.to_vec();
        tokio::task::spawn_blocking(move || hash_glob_patterns(&wd, &patterns)).await??
    };

    let file_count = file_hashes.len();
    for (path, hash) in &file_hashes {
        hasher.update(path.to_string_lossy().as_bytes());
        hasher.update(b":");
        hasher.update(hash.as_bytes());
        hasher.update(b"\x00");
    }

    let hash = hasher.finalize().to_hex().to_string();
    let cmd_preview = if step.run.len() > 50 {
        format!("{}...", &step.run[..50])
    } else {
        step.run.clone()
    };

    Ok(StepFingerprint {
        hash,
        inputs_summary: format!(
            "cmd=\"{}\" env_keys={} files={}",
            cmd_preview,
            sorted_env.len(),
            file_count,
        ),
    })
}

/// Walk a directory recursively (up to max_depth) and hash all files.
fn walk_and_hash(dir: &Path, max_depth: usize) -> anyhow::Result<BTreeMap<PathBuf, String>> {
    let mut results = BTreeMap::new();
    walk_recursive(dir, dir, max_depth, 0, &mut results)?;
    Ok(results)
}

fn walk_recursive(
    base: &Path,
    current: &Path,
    max_depth: usize,
    depth: usize,
    results: &mut BTreeMap<PathBuf, String>,
) -> anyhow::Result<()> {
    if depth > max_depth {
        return Ok(());
    }

    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return Ok(()), // Skip unreadable directories
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Skip hidden files/dirs and common build artifacts
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.')
                || name == "target"
                || name == "node_modules"
                || name == ".git"
            {
                continue;
            }
        }

        if path.is_file() {
            let relative = path.strip_prefix(base).unwrap_or(&path).to_owned();
            let hash = hash_file_sync(&path)?;
            results.insert(relative, hash);
        } else if path.is_dir() {
            walk_recursive(base, &path, max_depth, depth + 1, results)?;
        }
    }

    Ok(())
}

/// Hash files matching glob patterns relative to working_dir.
fn hash_glob_patterns(
    working_dir: &Path,
    patterns: &[String],
) -> anyhow::Result<BTreeMap<PathBuf, String>> {
    let mut results = BTreeMap::new();

    for pattern in patterns {
        let full_pattern = working_dir.join(pattern);
        let pattern_str = full_pattern.to_string_lossy().to_string();

        for entry in glob::glob(&pattern_str)? {
            let path = entry?;
            if path.is_file() {
                let relative = path
                    .strip_prefix(working_dir)
                    .unwrap_or(&path)
                    .to_owned();
                let hash = hash_file_sync(&path)?;
                results.insert(relative, hash);
            }
        }
    }

    Ok(results)
}

/// Hash a single file using blake3 with mmap for large files.
fn hash_file_sync(path: &Path) -> anyhow::Result<String> {
    let metadata = std::fs::metadata(path)?;

    if metadata.len() > 128 * 1024 {
        // Large file: use mmap + SIMD parallel hashing
        let mut hasher = Hasher::new();
        hasher.update_mmap(path)?;
        Ok(hasher.finalize().to_hex().to_string())
    } else {
        // Small file: read into memory (avoids mmap overhead for tiny files)
        let data = std::fs::read(path)?;
        Ok(blake3::hash(&data).to_hex().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_step(run: &str) -> StepConfig {
        StepConfig {
            name: "test".into(),
            image: None,
            run: run.into(),
            env: HashMap::new(),
            timeout_minutes: None,
            cache: None,
            artifacts: None,
        }
    }

    #[tokio::test]
    async fn test_fingerprint_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "hello").unwrap();

        let step = make_step("echo hello");
        let fp1 = compute_fingerprint(&step, dir.path(), &[]).await.unwrap();
        let fp2 = compute_fingerprint(&step, dir.path(), &[]).await.unwrap();

        assert_eq!(fp1.hash, fp2.hash, "Same inputs must produce same fingerprint");
    }

    #[tokio::test]
    async fn test_fingerprint_changes_with_command() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "hello").unwrap();

        let fp1 = compute_fingerprint(&make_step("echo hello"), dir.path(), &[])
            .await
            .unwrap();
        let fp2 = compute_fingerprint(&make_step("echo world"), dir.path(), &[])
            .await
            .unwrap();

        assert_ne!(fp1.hash, fp2.hash, "Different commands must produce different fingerprints");
    }

    #[tokio::test]
    async fn test_fingerprint_changes_with_file_content() {
        let dir = tempfile::tempdir().unwrap();
        let step = make_step("cargo build");

        std::fs::write(dir.path().join("src.rs"), "fn main() {}").unwrap();
        let fp1 = compute_fingerprint(&step, dir.path(), &[]).await.unwrap();

        std::fs::write(dir.path().join("src.rs"), "fn main() { println!(\"hi\"); }").unwrap();
        let fp2 = compute_fingerprint(&step, dir.path(), &[]).await.unwrap();

        assert_ne!(fp1.hash, fp2.hash, "Changed file must change fingerprint");
    }

    #[tokio::test]
    async fn test_fingerprint_changes_with_env() {
        let dir = tempfile::tempdir().unwrap();

        let mut step1 = make_step("cargo build");
        step1.env.insert("RUST_LOG".into(), "info".into());

        let mut step2 = make_step("cargo build");
        step2.env.insert("RUST_LOG".into(), "debug".into());

        let fp1 = compute_fingerprint(&step1, dir.path(), &[]).await.unwrap();
        let fp2 = compute_fingerprint(&step2, dir.path(), &[]).await.unwrap();

        assert_ne!(fp1.hash, fp2.hash, "Different env must change fingerprint");
    }

    #[tokio::test]
    async fn test_fingerprint_with_glob_patterns() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("test.txt"), "not included").unwrap();

        let step = make_step("cargo build");
        let fp = compute_fingerprint(&step, dir.path(), &["*.rs".into()])
            .await
            .unwrap();

        assert!(fp.inputs_summary.contains("files=1"), "Should only match .rs files");
    }

    #[tokio::test]
    async fn test_fingerprint_summary() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();

        let mut step = make_step("cargo build --release");
        step.env.insert("CC".into(), "gcc".into());

        let fp = compute_fingerprint(&step, dir.path(), &[]).await.unwrap();

        assert!(fp.inputs_summary.contains("cmd="));
        assert!(fp.inputs_summary.contains("env_keys=1"));
        assert!(fp.inputs_summary.contains("files=2"));
    }
}
