//! DB pool + migration embed. Postgres.

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use std::str::FromStr;

pub async fn connect(db_url: &str) -> anyhow::Result<PgPool> {
    let opts = PgConnectOptions::from_str(db_url)?;

    let pool = PgPoolOptions::new()
        .max_connections(16)
        .min_connections(2)
        // Postgres already strong on transactions; keep sane defaults.
        .connect_with(opts)
        .await?;

    Ok(pool)
}

pub async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    // Embed directory migrations/ ke binary
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
