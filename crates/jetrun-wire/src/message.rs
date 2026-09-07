use rkyv::{Archive, Deserialize, Serialize};

/// All possible RPC requests across jetrun services.
/// Serialized with rkyv for zero-copy deserialization.
#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(compare(PartialEq), derive(Debug))]
pub enum WireRequest {
    // ── Engine → Worker ──
    ExecuteStep {
        step_id: String,
        build_id: String,
        command: String,
        image: Option<String>,
        env: Vec<(String, String)>,
        working_dir: Option<String>,
        timeout_secs: Option<u32>,
    },
    CancelStep {
        step_id: String,
    },
    WorkerStatus,

    // ── Any → Cache ──
    CacheGet {
        key: String,
        restore_keys: Vec<String>,
    },
    CachePut {
        key: String,
        data: Vec<u8>,
    },
    CacheHas {
        key: String,
    },
    CacheEvict {
        key: String,
    },
    CacheStats,

    // ── Gateway → Auth ──
    ValidateToken {
        token: String,
    },
    ValidateApiKey {
        api_key: String,
    },
    CheckPermission {
        user_id: String,
        permission: String,
    },

    // ── Gateway → Engine ──
    ScheduleBuild {
        pipeline_id: String,
        trigger: String,
        commit_sha: Option<String>,
        branch: Option<String>,
    },
    CancelBuild {
        build_id: String,
    },
    GetBuildStatus {
        build_id: String,
    },
}

/// All possible RPC responses.
#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(compare(PartialEq), derive(Debug))]
pub enum WireResponse {
    // ── Generic ──
    Ok,
    Error {
        code: u16,
        message: String,
    },

    // ── Cache ──
    CacheHit {
        data: Vec<u8>,
        compression: u8, // 0=none, 1=zstd, 2=lz4
    },
    CacheMiss,
    CacheStatsResult {
        total_entries: u64,
        total_size_bytes: u64,
        hit_count: u64,
        miss_count: u64,
        hit_rate_pct: u32, // 0-10000 (two decimal precision)
    },

    // ── Auth ──
    TokenValid {
        user_id: String,
        email: String,
        username: String,
        role: String,
        permissions: Vec<String>,
    },
    TokenInvalid,
    PermissionGranted,
    PermissionDenied,

    // ── Engine ──
    BuildScheduled {
        build_id: String,
        build_number: u64,
    },
    BuildStatus {
        build_id: String,
        status: String,
        stages: Vec<StageStatusWire>,
    },

    // ── Worker ──
    StepOutput {
        step_id: String,
        stream: u8, // 0=stdout, 1=stderr
        content: Vec<u8>,
    },
    StepCompleted {
        step_id: String,
        exit_code: i32,
        duration_ms: u64,
    },
    WorkerStatusResult {
        active_steps: u32,
        max_concurrent: u32,
    },
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(compare(PartialEq), derive(Debug))]
pub struct StageStatusWire {
    pub name: String,
    pub status: String,
    pub steps: Vec<StepStatusWire>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
#[rkyv(compare(PartialEq), derive(Debug))]
pub struct StepStatusWire {
    pub name: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub cache_hit: bool,
}
