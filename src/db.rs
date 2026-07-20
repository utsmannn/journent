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

/// Audit event — fire-and-forget. Every mutation handler should call this
/// AFTER the mutation succeeds. A failure here never fails the request;
/// it only logs a warning. Survives container recreates (unlike stdout logs).
pub async fn log_event(
    pool: &PgPool,
    actor: &str,
    action: &str,
    target: Option<&str>,
    meta: serde_json::Value,
) {
    let r = sqlx::query(
        "INSERT INTO audit_log (actor, action, target, meta_json) VALUES ($1, $2, $3, $4)",
    )
    .bind(actor)
    .bind(action)
    .bind(target)
    .bind(meta)
    .execute(pool)
    .await;
    if let Err(e) = r {
        tracing::warn!(error = %e, action, "audit_log insert failed");
    }
}
