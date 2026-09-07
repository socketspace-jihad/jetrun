use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_gateway_host")]
    pub host: String,
    #[serde(default = "default_gateway_port")]
    pub port: u16,
    pub database_url: String,
    pub redis_url: Option<String>,
    pub engine_url: String,
    pub worker_url: String,
    pub cache_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EngineConfig {
    #[serde(default = "default_engine_host")]
    pub host: String,
    #[serde(default = "default_engine_port")]
    pub port: u16,
    pub database_url: String,
    pub worker_url: String,
    pub cache_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkerConfig {
    #[serde(default = "default_worker_host")]
    pub host: String,
    #[serde(default = "default_worker_port")]
    pub port: u16,
    pub cache_url: String,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_steps: u32,
    #[serde(default = "default_workspace_dir")]
    pub workspace_dir: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_cache_host")]
    pub host: String,
    #[serde(default = "default_cache_port")]
    pub port: u16,
    pub redis_url: Option<String>,
    #[serde(default = "default_cache_dir")]
    pub cache_dir: String,
    #[serde(default = "default_max_cache_size")]
    pub max_size_bytes: u64,
}

fn default_gateway_host() -> String {
    "0.0.0.0".into()
}
fn default_gateway_port() -> u16 {
    8080
}
fn default_engine_host() -> String {
    "0.0.0.0".into()
}
fn default_engine_port() -> u16 {
    9001
}
fn default_worker_host() -> String {
    "0.0.0.0".into()
}
fn default_worker_port() -> u16 {
    9002
}
fn default_cache_host() -> String {
    "0.0.0.0".into()
}
fn default_cache_port() -> u16 {
    9003
}
fn default_max_concurrent() -> u32 {
    4
}
fn default_workspace_dir() -> String {
    "/tmp/jetrun/workspace".into()
}
fn default_cache_dir() -> String {
    "/tmp/jetrun/cache".into()
}
fn default_max_cache_size() -> u64 {
    10 * 1024 * 1024 * 1024 // 10 GB
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_auth_host")]
    pub host: String,
    #[serde(default = "default_auth_port")]
    pub port: u16,
    pub database_url: String,
    pub jwt_secret: String,
    #[serde(default = "default_jwt_access_ttl")]
    pub jwt_access_ttl_secs: u64,
    #[serde(default = "default_jwt_refresh_ttl")]
    pub jwt_refresh_ttl_days: u64,
    #[serde(default = "default_superadmin_email")]
    pub superadmin_email: String,
    #[serde(default = "default_superadmin_password")]
    pub superadmin_password: String,
}

fn default_auth_host() -> String {
    "0.0.0.0".into()
}
fn default_auth_port() -> u16 {
    9004
}
fn default_jwt_access_ttl() -> u64 {
    900 // 15 minutes
}
fn default_jwt_refresh_ttl() -> u64 {
    30 // 30 days
}
fn default_superadmin_email() -> String {
    "admin@jetrun.local".into()
}
fn default_superadmin_password() -> String {
    "changeme".into()
}
