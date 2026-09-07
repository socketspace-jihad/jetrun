use serde::{Deserialize, Serialize};

/// A content-based fingerprint for a build step.
///
/// The fingerprint is computed as:
///   blake3(command || sorted_env || sorted_input_file_hashes)
///
/// If a step's fingerprint matches a previously cached execution,
/// the step output can be restored from cache — skipping execution entirely.
/// This is what makes incremental builds 10x faster.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct StepFingerprint {
    /// blake3 hash of all inputs
    pub hash: String,
    /// Human-readable summary: "cmd=..., env_keys=N, files=N"
    pub inputs_summary: String,
}

impl StepFingerprint {
    /// Cache key used in the cache service for fingerprint lookups
    pub fn cache_key(&self) -> String {
        format!("fingerprint:{}", self.hash)
    }
}
