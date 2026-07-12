//! AppState shared across handlers.

use sqlx::PgPool;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub cfg: &'static Config,
}
