//! Human web session (cookie → DB-backed session row).
//! Cookie name: `journent_sid` (HttpOnly). Session row carries csrf_token.

use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::HeaderMap;
use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::HumanSession;

pub const COOKIE_NAME: &str = "journent_sid";
pub const SESSION_TTL_DAYS: i64 = 30;

/// Create a session row, return (session_id, csrf_token).
pub async fn create_session(
    pool: &PgPool,
    human_id: Uuid,
    ua: Option<String>,
    ip: Option<String>,
) -> anyhow::Result<(Uuid, String)> {
    let id = Uuid::new_v4();
    let csrf = random_csrf();
    let expires = Utc::now() + Duration::days(SESSION_TTL_DAYS);
    sqlx::query(
        "INSERT INTO human_sessions (id, human_id, csrf_token, user_agent, ip, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(human_id)
    .bind(&csrf)
    .bind(ua.as_deref())
    .bind(ip.as_deref())
    .bind(expires)
    .execute(pool)
    .await?;
    Ok((id, csrf))
}

pub async fn lookup_session(pool: &PgPool, sid: Uuid) -> anyhow::Result<Option<HumanSession>> {
    let row = sqlx::query_as::<_, HumanSession>(
        "SELECT * FROM human_sessions WHERE id = $1 AND expires_at > NOW()",
    )
    .bind(sid)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn destroy(pool: &PgPool, sid: Uuid) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM human_sessions WHERE id = $1")
        .bind(sid)
        .execute(pool)
        .await?;
    Ok(())
}

/// Read cookie value (session id) from request headers.
pub fn read_cookie(headers: &HeaderMap) -> Option<Uuid> {
    let raw = headers.get(COOKIE)?.to_str().ok()?;
    for kv in raw.split(';') {
        let kv = kv.trim();
        if let Some(rest) = kv.strip_prefix(&format!("{}=", COOKIE_NAME)) {
            if let Ok(id) = Uuid::parse_str(rest) {
                return Some(id);
            }
        }
    }
    None
}

/// Build a Set-Cookie header value.
pub fn set_cookie_value(sid: &str, max_age_seconds: i64) -> String {
    format!(
        "{}={}; HttpOnly; Path=/; SameSite=Lax; Max-Age={}",
        COOKIE_NAME, sid, max_age_seconds
    )
}

pub fn clear_cookie_value() -> String {
    format!("{}=deleted; HttpOnly; Path=/; SameSite=Lax; Max-Age=0", COOKIE_NAME)
}

fn random_csrf() -> String {
    let mut b = [0u8; 24];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut b);
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, b)
}

/// middleware-friendly: append SET_COOKIE header on response (append, so multi-cookie works).
pub fn attach_set_cookie(headers: &mut HeaderMap, sid: &Uuid) {
    headers.append(
        SET_COOKIE,
        set_cookie_value(&sid.to_string(), SESSION_TTL_DAYS * 86400)
            .parse()
            .unwrap(),
    );
}

pub fn attach_clear_cookie(headers: &mut HeaderMap) {
    headers.append(SET_COOKIE, clear_cookie_value().parse().unwrap());
}

/// Re-export base64 engine access for other modules.
#[allow(unused_imports)]
pub use base64::Engine as _;
