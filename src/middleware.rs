//! Middleware + extractors.
//!
//! - `WebUser` / `WebCsrf`: human web session, populated by `load_web_user` middleware.
//! - `AgentAuth`: agent bearer-key extractor (used by /api authed handlers).

use axum::extract::{FromRef, FromRequestParts, Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use std::sync::Arc;

use crate::auth::session;
use crate::error::AppError;
use crate::models::Human;
use crate::state::AppState;

/// Loaded human (if any) from session cookie. Populated by `load_web_user`.
#[derive(Clone, Default)]
pub struct WebUser(pub Option<Human>);

/// Loaded CSRF token for the current session (for form actions).
#[derive(Clone, Default)]
pub struct WebCsrf(pub Option<String>);

/// Insert WebUser/WebCsrf extensions (always ok; sets None if no/invalid session).
pub async fn load_web_user(
    State(st): State<AppState>,
    headers: HeaderMap,
    mut req: Request,
    next: Next,
) -> Response {
    let (user, csrf) = match session::read_cookie(&headers) {
        Some(sid) => match session::lookup_session(&st.pool, sid).await {
            Ok(Some(s)) => {
                let csrf = s.csrf_token.clone();
                let h: Option<Human> =
                    sqlx::query_as::<_, Human>("SELECT * FROM humans WHERE id = $1")
                        .bind(s.human_id)
                        .fetch_optional(&st.pool)
                        .await
                        .ok()
                        .flatten();
                (h, Some(csrf))
            }
            _ => (None, None),
        },
        None => (None, None),
    };
    req.extensions_mut().insert(WebUser(user));
    req.extensions_mut().insert(WebCsrf(csrf));
    next.run(req).await
}

// `Human` is used in the lookup above; keep the import even if a later refactor moves it.
#[allow(dead_code)]
fn _keep_human(_h: Human) {}

/// Agent injected as an extractor: reads Authorization: Bearer <key> and verifies.
#[derive(Clone)]
pub struct AgentAuth(pub Arc<crate::models::Agent>);

#[axum::async_trait]
impl<S> FromRequestParts<S> for AgentAuth
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let st = AppState::from_ref(state);
        let token =
            crate::auth::agent_key::extract_bearer(&parts.headers).ok_or(AppError::Unauthorized)?;
        let hash = crate::auth::agent_key::hash_key(&token);
        let agent: Option<crate::models::Agent> =
            sqlx::query_as::<_, crate::models::Agent>("SELECT * FROM agents WHERE key_hash = $1")
                .bind(&hash)
                .fetch_optional(&st.pool)
                .await?;
        match agent {
            Some(a) => Ok(AgentAuth(Arc::new(a))),
            None => Err(AppError::Unauthorized),
        }
    }
}
