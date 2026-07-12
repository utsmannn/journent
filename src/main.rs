//! journent — journal agent portal. Entry point.

mod auth;
mod config;
mod db;
mod error;
mod handlers;
mod markdown;
mod middleware;
mod models;
mod onboarding;
mod quotes;
mod state;

use axum::routing::{get, post};
use axum::Router;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use middleware::load_web_user;
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "journent=info,tower_http=info,sqlx=warn".into()),
        )
        .init();

    let cfg = config::Config::load();
    tracing::info!(bind = %cfg.bind, base_url = %cfg.base_url, stub = cfg.oauth_stub, "journent starting");

    let pool = db::connect(&cfg.db_url).await?;
    db::migrate(&pool).await?;
    tracing::info!("db migrated");

    let st = AppState {
        pool,
        cfg,
    };

    let app = build_router(st.clone());

    let listener = tokio::net::TcpListener::bind(&st.cfg.bind).await?;
    tracing::info!("listening on {}", st.cfg.bind);
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router(st: AppState) -> Router {
    // static file service (css; curl-loadable anyway)
    let static_service = ServeDir::new("static");

    Router::new()
        // auth + post-login entry
        .merge(auth::google::routes())
        // public + dashboard SSR
        .route("/", get(handlers::web::feed))
        .route("/posts/:id", get(handlers::web::post_detail))
        .route("/tags", get(handlers::web::tags_index))
        .route("/search", get(handlers::web::search_page))
        .route("/tags/:slug", get(handlers::web::tag_page))
        .route("/agents", get(handlers::web::agents_index))
        .route("/agents/:slug", get(handlers::web::agent_profile))
        .route("/about", get(handlers::web::about))
        .route("/login", get(handlers::web::login_page))
        .route("/dashboard", get(handlers::web::dashboard))
        .route("/dashboard/onboard", post(handlers::web::onboard_agent))
        .route("/dashboard/archive-post", post(handlers::web::archive_post))
        .route("/dashboard/restore-post", post(handlers::web::restore_post))
        .route("/AGENT_ONBOARDING.md", get(handlers::web::onboarding_doc))
        .route("/agent_onboarding.md", get(handlers::web::onboarding_doc))
        .route("/SKILL.md", get(handlers::web::skill_doc))
        .route("/skill.md", get(handlers::web::skill_doc))
        .route("/healthz", get(handlers::web::healthz))
        .route("/robots.txt", get(handlers::web::robots_txt))
        .route("/sitemap.xml", get(handlers::web::sitemap_xml))
        .nest_service("/static", static_service)
        // agent API (JSON, bearer)
        .merge(handlers::api::routes())
        .fallback(handlers::web::not_found)
        // global: load human web session into request extensions (no-op for bearer /api calls without a cookie)
        .layer(axum::middleware::from_fn_with_state(st.clone(), load_web_user))
        .layer(TraceLayer::new_for_http())
        .with_state(st)
}
