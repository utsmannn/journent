//! Google OAuth login (real) + stub login (dev).
//!
//! Stub mode (cfg.oauth_stub or no client id):
//!   GET /auth/stub?email=foo@bar.com&sub=stub-123 → create/find human → /dashboard/entry
//!
//! Real mode:
//!   GET /auth/google → redirect to Google consent.
//!   GET /auth/google/callback?code=... → exchange + fetch userinfo.
//!
//! Both converge at `/dashboard/entry?hid=<human_id>` which creates the session
//! cookie and first agent (if needed), then redirects to /dashboard.

use axum::extract::{Query, State};
use axum::http::header::LOCATION;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope, TokenResponse,
    TokenUrl,
};
use serde::Deserialize;

use crate::auth::session;
use crate::error::{AppError, AppResult};
use crate::models::Human;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/stub", get(stub_login))
        .route("/auth/google", get(google_start))
        .route("/auth/google/callback", get(google_callback))
        .route("/auth/logout", post(logout))
        .route("/dashboard/entry", get(entry_after_login))
}

#[derive(Deserialize)]
struct StubQuery {
    email: String,
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

async fn stub_login(
    State(st): State<AppState>,
    Query(q): Query<StubQuery>,
) -> AppResult<Redirect> {
    if !st.cfg.oauth_stub {
        return Err(AppError::BadRequest("stub login disabled".into()));
    }
    let sub = q.sub.unwrap_or_else(|| format!("stub-{}", q.email));
    let human = upsert_human(&st, &sub, &q.email, q.name).await?;
    Ok(Redirect::to(&format!("/dashboard/entry?hid={}", human.id)))
}

// ---------- Real OAuth ----------

fn build_client(cfg: &crate::config::Config) -> anyhow::Result<BasicClient> {
    let cid = cfg
        .google_client_id
        .clone()
        .ok_or_else(|| anyhow::anyhow!("google client id not set"))?;
    let secret = cfg
        .google_client_secret
        .clone()
        .ok_or_else(|| anyhow::anyhow!("google client secret not set"))?;

    let client = BasicClient::new(
        ClientId::new(cid),
        Some(ClientSecret::new(secret)),
        AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())?,
        Some(TokenUrl::new(
            "https://oauth2.googleapis.com/token".to_string(),
        )?),
    )
    .set_redirect_uri(RedirectUrl::new(format!(
        "{}/auth/google/callback",
        cfg.base_url
    ))?);
    Ok(client)
}

async fn google_start(State(st): State<AppState>) -> AppResult<Redirect> {
    if st.cfg.oauth_stub {
        return Ok(Redirect::to("/login?err=oauth+not+configured"));
    }
    let client = build_client(st.cfg).map_err(|e| AppError::internal(e.to_string()))?;
    let (auth_url, _csrf) = client
        .authorize_url(CsrfToken::new_random)
        // openid + email + profile scopes — we only need email to identify the human.
        .add_scope(Scope::new("openid".to_string()))
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .url();
    Ok(Redirect::to(auth_url.as_str()))
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: String,
    #[allow(dead_code)]
    #[serde(default)]
    state: Option<String>,
}

async fn google_callback(
    State(st): State<AppState>,
    Query(q): Query<CallbackQuery>,
) -> AppResult<Redirect> {
    if st.cfg.oauth_stub {
        return Err(AppError::BadRequest("oauth stub mode; use /auth/stub".into()));
    }
    let client = build_client(st.cfg).map_err(|e| AppError::internal(e.to_string()))?;

    let token = client
        .exchange_code(AuthorizationCode::new(q.code))
        .request_async(oauth2::reqwest::async_http_client)
        .await
        .map_err(|e| AppError::internal(format!("token exchange failed: {e}")))?;

    let access_token = token.access_token().secret().to_string();
    let resp = reqwest::Client::new()
        .get("https://www.googleapis.com/oauth2/v3/userinfo")
        .bearer_auth(&access_token)
        .send()
        .await
        .map_err(|e| AppError::internal(format!("userinfo fetch: {e}")))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| AppError::internal(format!("userinfo parse: {e}")))?;

    let sub = resp["sub"]
        .as_str()
        .ok_or_else(|| AppError::internal("no sub"))?
        .to_string();
    let email = resp["email"]
        .as_str()
        .ok_or_else(|| AppError::internal("no email"))?
        .to_string();
    let name = resp["name"].as_str().map(|s| s.to_string());

    let human = upsert_human(&st, &sub, &email, name).await?;
    Ok(Redirect::to(&format!("/dashboard/entry?hid={}", human.id)))
}

// ---------- shared identity upsert ----------

async fn upsert_human(
    st: &AppState,
    google_sub: &str,
    email: &str,
    name: Option<String>,
) -> AppResult<Human> {
    let human: Option<Human> =
        sqlx::query_as::<_, Human>("SELECT * FROM humans WHERE google_sub = $1")
            .bind(google_sub)
            .fetch_optional(&st.pool)
            .await?;

    if let Some(h) = human {
        sqlx::query(
            "UPDATE humans SET last_seen_at = NOW(), display_name = COALESCE($2, display_name) WHERE id = $1",
        )
        .bind(h.id)
        .bind(name)
        .execute(&st.pool)
        .await?;
        let h2: Human = sqlx::query_as::<_, Human>("SELECT * FROM humans WHERE id = $1")
            .bind(h.id)
            .fetch_one(&st.pool)
            .await?;
        Ok(h2)
    } else {
        let h: Human = sqlx::query_as::<_, Human>(
            "INSERT INTO humans (google_sub, email, display_name) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(google_sub)
        .bind(email)
        .bind(name)
        .fetch_one(&st.pool)
        .await?;
        Ok(h)
    }
}

// ---------- internal entry: create session + ensure first agent ----------

#[derive(Deserialize)]
struct EntryQuery {
    hid: uuid::Uuid,
}

/// After OAuth/stub resolves the human identity, redirect here with hid.
/// Creates a session cookie and ensures the human has at least one agent.
/// If a brand-new agent was created, the fresh key is revealed once via a
/// short-lived cookie so the dashboard can render the onboarding box.
async fn entry_after_login(
    State(st): State<AppState>,
    Query(q): Query<EntryQuery>,
) -> AppResult<Response> {
    let human: Human = sqlx::query_as::<_, Human>("SELECT * FROM humans WHERE id = $1")
        .bind(q.hid)
        .fetch_one(&st.pool)
        .await
        .map_err(|_| AppError::NotFound)?;

    let (sid, _csrf) = session::create_session(&st.pool, human.id, None, None)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;

    let agent_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents WHERE human_id = $1")
        .bind(human.id)
        .fetch_one(&st.pool)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;

    let reveal_cookie: Option<String> = if agent_count == 0 {
        let (full_key, display_prefix) =
            crate::auth::agent_key::generate_full_key(&st.cfg.key_prefix);
        let key_hash = crate::auth::agent_key::hash_key(&full_key);
        let slug = format!("unnamed-{}", &uuid::Uuid::new_v4().to_string()[..8]);
        sqlx::query(
            "INSERT INTO agents (human_id, name, slug, key_hash, key_prefix)
             VALUES ($1, 'Unnamed Agent', $2, $3, $4)",
        )
        .bind(human.id)
        .bind(&slug)
        .bind(&key_hash)
        .bind(&display_prefix)
        .execute(&st.pool)
        .await
        .map_err(|e| AppError::internal(e.to_string()))?;

        Some(format!(
            "journent_reveal={}; HttpOnly; Path=/; SameSite=Lax; Max-Age=600",
            full_key
        ))
    } else {
        None
    };

    let mut resp = (StatusCode::SEE_OTHER, "").into_response();
    resp.headers_mut().insert(LOCATION, "/dashboard".parse().unwrap());
    session::attach_set_cookie(resp.headers_mut(), &sid);
    if let Some(c) = reveal_cookie {
        resp.headers_mut()
            .append(axum::http::header::SET_COOKIE, c.parse().unwrap());
    }
    Ok(resp)
}

async fn logout(State(st): State<AppState>, headers: HeaderMap) -> AppResult<Response> {
    if let Some(sid) = session::read_cookie(&headers) {
        let _ = session::destroy(&st.pool, sid).await;
    }
    let mut resp = (StatusCode::SEE_OTHER, "").into_response();
    resp.headers_mut().insert(LOCATION, "/".parse().unwrap());
    session::attach_clear_cookie(resp.headers_mut());
    Ok(resp)
}
