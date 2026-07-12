//! journent — config from env. One source of truth.

use std::env;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct Config {
    pub base_url: String,
    pub bind: String,
    pub db_url: String,
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    pub oauth_stub: bool,
    pub session_key: String,
    pub key_prefix: String,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

impl Config {
    pub fn load() -> &'static Config {
        CONFIG.get_or_init(load_from_env)
    }
}

fn load_from_env() -> Config {
    let _ = dotenvy::dotenv();

    let oauth_stub = env::var("JOURNENT_OAUTH_STUB")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let google_client_id = env::var("GOOGLE_CLIENT_ID").ok().filter(|s| !s.is_empty());
    let google_client_secret = env::var("GOOGLE_CLIENT_SECRET").ok().filter(|s| !s.is_empty());

    // compute stub flag BEFORE moving google_client_id into the struct
    let oauth_stub = oauth_stub || google_client_id.is_none();

    Config {
        base_url: env::var("JOURNENT_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:8080".into())
            .trim_end_matches('/')
            .to_string(),
        bind: env::var("JOURNENT_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into()),
        db_url: env::var("JOURNENT_DB_URL")
            .unwrap_or_else(|_| "postgres://journent:journent@db:5432/journent".into()),
        google_client_id,
        google_client_secret,
        oauth_stub,
        session_key: env::var("JOURNENT_SESSION_KEY")
            .unwrap_or_else(|_| "dev-insecure-key-change-me".into()),
        key_prefix: env::var("JOURNENT_KEY_PREFIX")
            .unwrap_or_else(|_| "jrn".into()),
    }
}
