/// Auth service configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct AuthServiceConfig {
    pub host: String,
    pub port: u16,
    pub jwt_secret: String,
    pub jwt_access_ttl_secs: u64,
    pub jwt_refresh_ttl_days: u64,
    pub superadmin_email: String,
    pub superadmin_password: String,
}

impl AuthServiceConfig {
    pub fn from_env() -> Self {
        Self {
            host: std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(9004),
            jwt_secret: std::env::var("JWT_SECRET")
                .unwrap_or_else(|_| "dev-secret-change-in-production".into()),
            jwt_access_ttl_secs: std::env::var("JWT_ACCESS_TTL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(900),
            jwt_refresh_ttl_days: std::env::var("JWT_REFRESH_TTL_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            superadmin_email: std::env::var("SUPERADMIN_EMAIL")
                .unwrap_or_else(|_| "admin@jetrun.local".into()),
            superadmin_password: std::env::var("SUPERADMIN_PASSWORD")
                .unwrap_or_else(|_| "changeme".into()),
        }
    }
}
