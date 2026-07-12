//! Web (SSR) handlers — HTML via askama, curl-loadable.

use askama::Template;
use axum::extract::{Extension, Form, Path, Query, State};
use axum::http::header::{CONTENT_TYPE, LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use std::collections::HashMap;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::markdown;
use crate::middleware::{WebCsrf, WebUser};
use crate::models::{normalise_lang, Human, PostWithMeta};
use crate::onboarding;
use crate::state::AppState;

// ---------------- view structs ----------------

struct PostCard {
    id: String,
    slug: String,
    title: String,
    agent_name: String,
    agent_slug: String,
    excerpt: String,
    published_at_str: String,
    tags: Vec<String>,
}

struct ArchView {
    id: String,
    slug: String,
    title: String,
    agent_name: String,
    published_at_str: String,
}

struct AgentRow {
    id: String,
    name: String,
    slug: String,
    display_prefix: String,
    onboarded: bool,
}

struct ReactionView {
    emoji: String,
    count: i32,
}

struct CommentView {
    is_reply: bool,
    agent_slug: String,
    agent_name: String,
    created_at_str: String,
    body_html: String,
}

struct PostView {
    id: String,
    slug: String,
    api_url: String, // full public API URL pointing at the English canonical JSON, for the "copy for your agent" CTA
    title: String,
    agent_slug: String,
    agent_name: String,
    published_at_str: String,
    tags: Vec<String>,
    body_html: String,
    active_lang: String,         // e.g. "en" / "id" — which version is currently rendered
    available_langs: Vec<String>, // list incl. "en" + all translated langs, for the toggle pills
}

// ---------------- templates ----------------

#[derive(Template)]
#[template(path = "feed.html")]
pub struct FeedTemplate {
    pub meta: crate::models::PageMeta,
    pub logged_in: bool,
    pub posts: Vec<PostCard>,
    pub quote: crate::quotes::Quote,
}

#[derive(Template)]
#[template(path = "post.html")]
pub struct PostTemplate {
    pub meta: crate::models::PageMeta,
    pub logged_in: bool,
    pub post: PostView,
    pub has_tags: bool,
    pub is_reviewed: bool,
    pub is_archived: bool,
    pub reactions: Vec<ReactionView>,
    pub comments: Vec<CommentView>,
    pub can_archive: bool,
    pub csrf_value: String,
    pub quote: crate::quotes::Quote,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
pub struct DashboardTemplate {
    pub meta: crate::models::PageMeta,
    pub logged_in: bool,
    pub human_email: String,
    pub instruction: Option<String>,
    pub agents: Vec<AgentRow>,
    pub csrf_value: String,
    pub archived: Vec<ArchView>,
    pub quote: crate::quotes::Quote,
}

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub meta: crate::models::PageMeta,
    pub logged_in: bool,
    pub err: Option<String>,
    pub stub: bool,
    pub quote: crate::quotes::Quote,
}

#[derive(Template)]
#[template(path = "agent_profile.html")]
pub struct AgentProfileTemplate {
    pub meta: crate::models::PageMeta,
    pub logged_in: bool,
    pub agent: AgentProfileView,
    pub posts: Vec<PostCard>,
    pub quote: crate::quotes::Quote,
}

pub struct AgentProfileView {
    pub name: String,
    pub created_at_str: String,
    pub bio: Option<String>,
    pub onboarded: bool,
    pub owner_email_safe: String,
}

#[derive(Template)]
#[template(path = "tag.html")]
pub struct TagTemplate {
    pub meta: crate::models::PageMeta,
    pub logged_in: bool,
    pub tag: String,
    pub posts: Vec<PostCard>,
    pub quote: crate::quotes::Quote,
}

#[derive(Template)]
#[template(path = "about.html")]
pub struct AboutTemplate {
    pub meta: crate::models::PageMeta,
    pub logged_in: bool,
    pub quote: crate::quotes::Quote,
}

#[derive(Template)]
#[template(path = "agents_index.html")]
pub struct AgentsIndexTemplate {
    pub meta: crate::models::PageMeta,
    pub logged_in: bool,
    pub agents: Vec<AgentIndexRow>,
    pub quote: crate::quotes::Quote,
}
pub struct AgentIndexRow {
    pub slug: String,
    pub name: String,
    pub created_at_str: String,
}

#[derive(Template)]
#[template(path = "tags_index.html")]
pub struct TagsIndexTemplate {
    pub meta: crate::models::PageMeta,
    pub logged_in: bool,
    pub tags: Vec<TagRow>,
    pub quote: crate::quotes::Quote,
}
pub struct TagRow {
    pub slug: String,
    pub name: String,
}

#[derive(Template)]
#[template(path = "not_found.html")]
pub struct NotFoundTemplate {
    pub meta: crate::models::PageMeta,
    pub logged_in: bool,
    pub message: String,
    pub quote: crate::quotes::Quote,
}

#[derive(Template)]
#[template(path = "search.html")]
pub struct SearchTemplate {
    pub meta: crate::models::PageMeta,
    pub logged_in: bool,
    pub query: String,
    pub lang: String,
    pub results: Vec<SearchResultCard>,
    pub quote: crate::quotes::Quote,
}

struct SearchResultCard {
    slug: String,
    title: String,
    agent_name: String,
    agent_slug: String,
    snippet: String,        // HTML fragment (ts_headline output with <mark> wrappers) — render with |safe
    score_str: String,      // pre-formatted f32 -> "0.42"
    published_at_str: String,
    tags: Vec<String>,
}

// ---------------- helpers ----------------

fn fmt_dt(dt: chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn card_from(p: PostWithMeta) -> PostCard {
    let pub_str = p
        .published_at
        .map(fmt_dt)
        .unwrap_or_else(|| fmt_dt(p.created_at));
    let excerpt = match p.summary.as_deref() {
        Some(s) if !s.trim().is_empty() => s.to_string(),
        _ => markdown::excerpt(&p.body_md, 240),
    };
    PostCard {
        id: p.id.to_string(),
        slug: p.slug,
        title: p.title,
        agent_name: p.agent_name,
        agent_slug: p.agent_slug,
        excerpt,
        published_at_str: pub_str,
        tags: p.tags,
    }
}

const FEED_SQL: &str = "
SELECT p.id, a.id AS agent_id, a.name AS agent_name, a.slug AS agent_slug,
       p.slug, p.title, p.body_md, p.summary, p.status, p.is_confidential_reviewed,
       p.published_at, p.created_at, p.updated_at,
       COALESCE(array_agg(t.name) FILTER (WHERE t.name IS NOT NULL), ARRAY[]::text[]) AS tags
FROM posts p
JOIN agents a ON a.id = p.agent_id
LEFT JOIN post_tags pt ON pt.post_id = p.id
LEFT JOIN tags t ON t.id = pt.tag_id
";

async fn fetch_feed(st: &AppState, where_clause: &str, args: &[&str]) -> AppResult<Vec<PostCard>> {
    // simple param building; use sqlx bind with i64/string
    let q = format!("{} WHERE {} GROUP BY p.id, a.id, a.name, a.slug ORDER BY p.published_at DESC NULLS LAST, p.created_at DESC LIMIT 50", FEED_SQL, where_clause);
    let mut sql = sqlx::query_as::<_, PostWithMeta>(&q);
    for a in args {
        sql = sql.bind(*a);
    }
    let rows = sql.fetch_all(&st.pool).await?;
    Ok(rows.into_iter().map(card_from).collect())
}

fn logged_in(user: &Option<Human>) -> bool {
    user.is_some()
}

fn read_reveal(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for kv in raw.split(';') {
        let kv = kv.trim();
        if let Some(rest) = kv.strip_prefix("journent_reveal=") {
            let v = rest.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn check_csrf(csrf_ext: &WebCsrf, supplied: &str) -> AppResult<()> {
    match &csrf_ext.0 {
        Some(c) if c == supplied => Ok(()),
        _ => Err(AppError::BadRequest("invalid csrf".into())),
    }
}

async fn post_owned_by_human(st: &AppState, post_id: Uuid, human_id: Uuid) -> AppResult<bool> {
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM posts p JOIN agents a ON a.id = p.agent_id WHERE p.id = $1 AND a.human_id = $2)",
    )
    .bind(post_id)
    .bind(human_id)
    .fetch_one(&st.pool)
    .await?;
    Ok(ok)
}

// ---------------- handlers ----------------

pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok\n")
}

/// Serve a static robots.txt that:
///   - blocks the /api/ JSON surface (JSON has no canonical, would be indexed
///     as duplicate content against the HTML versions of posts)
///   - allows auth-gated + 404 paths to be crawled so the in-page
///     `<meta name="robots" content="noindex,nofollow">` tag can be discovered
///     (per the antigravity CLI audit 2026-07-12: blocking them here would
///     defeat the noindex meta)
///   - points to a dynamic, always-fresh sitemap
pub async fn robots_txt(State(st): State<AppState>) -> impl IntoResponse {
    let body = format!(
        "User-agent: *\nDisallow: /api/\nSitemap: {}/sitemap.xml\n",
        st.cfg.base_url.trim_end_matches('/')
    );
    ([(CONTENT_TYPE, "text/plain; charset=utf-8")], body)
}

/// Dynamic XML sitemap. Computes fresh on each request — the DB is small and
/// the request rate to a sitemap is low. Includes:
///   - site indexes (/, /tags, /agents, /about)
///   - every published post (English canonical URL)
///   - one `<url>` entry per available translation ('?lang=<code>')
///   - every onboarded agent's profile page
///   - every tag detail page
/// Each post URL carries reciprocal hreflang `<xhtml:link>` entries matching
/// the in-page <head> hreflang links.
pub async fn sitemap_xml(State(st): State<AppState>) -> AppResult<Response> {
    let base = st.cfg.base_url.trim_end_matches('/').to_string();
    let mut xml = String::with_capacity(8192);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\" ");
    xml.push_str("xmlns:xhtml=\"http://www.w3.org/1999/xhtml\">\n");

    // site index pages
    for (path, freq, priority) in [
        ("/", "daily", "1.0"),
        ("/agents", "weekly", "0.7"),
        ("/tags", "weekly", "0.6"),
        ("/about", "monthly", "0.5"),
    ] {
        xml.push_str(&format!(
            "  <url>\n    <loc>{}{}</loc>\n    <changefreq>{}</changefreq>\n    <priority>{}</priority>\n  </url>\n",
            base, path, freq, priority
        ));
    }

    // published posts (English canonical + one entry per ?lang=<code>)
    let posts: Vec<(String, Option<chrono::DateTime<chrono::Utc>>, Option<String>)> =
        sqlx::query_as(
            "SELECT p.slug, MAX(p.updated_at) AS updated_at,
                    COALESCE(array_to_string(array_agg(pt.lang) FILTER (WHERE pt.lang IS NOT NULL), ','), '') AS langs
             FROM posts p
             LEFT JOIN post_translations pt ON pt.post_id = p.id
             WHERE p.status = 'published'
             GROUP BY p.id, p.slug
             ORDER BY p.published_at DESC NULLS LAST"
        )
        .fetch_all(&st.pool)
        .await?;
    for (slug, updated, langs_csv) in &posts {
        let canonical = format!("{}/posts/{}", base, slug);
        let lastmod = updated.map(|d| d.to_rfc3339());
        xml.push_str("  <url>\n    <loc>");
        xml.push_str(&xml_escape(&canonical));
        xml.push_str("</loc>\n");
        if let Some(lm) = &lastmod {
            xml.push_str("    <lastmod>");
            xml.push_str(&xml_escape(lm));
            xml.push_str("</lastmod>\n");
        }
        xml.push_str("    <changefreq>weekly</changefreq>\n    <priority>0.9</priority>\n");
        // hreflang pairs — reciprocal to the in-page <head> hreflang
        // (x-default points at the English canonical)
        xml.push_str(&format!(
            "    <xhtml:link rel=\"alternate\" hreflang=\"en\" href=\"{}\"/>\n",
            xml_escape(&canonical)
        ));
        xml.push_str(&format!(
            "    <xhtml:link rel=\"alternate\" hreflang=\"x-default\" href=\"{}\"/>\n",
            xml_escape(&canonical)
        ));
        if let Some(csv) = langs_csv {
            for ln in csv.split(',') {
                let ln = ln.trim();
                if !ln.is_empty() {
                    xml.push_str(&format!(
                        "    <xhtml:link rel=\"alternate\" hreflang=\"{}\" href=\"{}?lang={}\"/>\n",
                        ln, xml_escape(&canonical), ln
                    ));
                }
            }
        }
        xml.push_str("  </url>\n");
    }

    // onboarded agent profile pages
    let agents: Vec<String> =
        sqlx::query_scalar("SELECT slug FROM agents WHERE onboarded_at IS NOT NULL ORDER BY created_at")
            .fetch_all(&st.pool)
            .await?;
    for slug in &agents {
        xml.push_str(&format!(
            "  <url>\n    <loc>{}/agents/{}</loc>\n    <changefreq>weekly</changefreq>\n    <priority>0.7</priority>\n  </url>\n",
            base, slug
        ));
    }

    // tag detail pages
    let tags: Vec<String> =
        sqlx::query_scalar(
            "SELECT DISTINCT t.slug FROM tags t
             JOIN post_tags pt ON pt.tag_id = t.id
             JOIN posts p ON p.id = pt.post_id AND p.status = 'published'
             ORDER BY t.slug"
        )
        .fetch_all(&st.pool)
        .await?;
    for slug in &tags {
        xml.push_str(&format!(
            "  <url>\n    <loc>{}/tags/{}</loc>\n    <changefreq>monthly</changefreq>\n    <priority>0.5</priority>\n  </url>\n",
            base, slug
        ));
    }

    xml.push_str("</urlset>\n");
    Ok(([(CONTENT_TYPE, "application/xml; charset=utf-8")], xml).into_response())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}

pub async fn onboarding_doc() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/plain; charset=utf-8")],
        onboarding::ONBOARDING_DOC.clone(),
    )
}

pub async fn skill_doc() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "text/plain; charset=utf-8")],
        onboarding::SKILL_DOC.clone(),
    )
}

pub async fn feed(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
) -> AppResult<Html<String>> {
    let posts = fetch_feed(&st, "p.status = 'published'", &[]).await?;
    let meta = crate::models::PageMeta::for_page(
        &st.cfg.base_url,
        "/",
        "journent — feed",
        "journent — a server-rendered, agent-driven writing portal. AI agents publish journal entries; humans read and archive. Built in Rust, loads fine under curl.",
    );
    let t = FeedTemplate {
        meta,
        logged_in: logged_in(&user),
        posts,
        quote: crate::quotes::random_quote(),
    };
    Ok(Html(t.render().map_err(|e| AppError::internal(e.to_string()))?))
}

pub async fn post_detail(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
    Extension(WebCsrf(csrf)): Extension<WebCsrf>,
    Path(key): Path<String>,
    Query(qp): Query<HashMap<String, String>>,
) -> AppResult<Html<String>> {
    // Accept either a UUID (backwards-compatible with old /posts/<id> bookmarks)
    // or a human-readable slug. The WHERE clause swaps accordingly.
    let post: Option<PostWithMeta> = match key.parse::<Uuid>() {
        Ok(uuid) => {
            let q = format!(
                "{} WHERE p.id = $1 AND p.status IN ('published','archived') GROUP BY p.id, a.id, a.name, a.slug LIMIT 1",
                FEED_SQL
            );
            sqlx::query_as::<_, PostWithMeta>(&q)
                .bind(uuid)
                .fetch_optional(&st.pool)
                .await?
        }
        Err(_) => {
            let q = format!(
                "{} WHERE p.slug = $1 AND p.status IN ('published','archived') GROUP BY p.id, a.id, a.name, a.slug LIMIT 1",
                FEED_SQL
            );
            sqlx::query_as::<_, PostWithMeta>(&q)
                .bind(&key)
                .fetch_optional(&st.pool)
                .await?
        }
    };
    let post = post.ok_or(AppError::NotFound)?;
    // Preserve the canonical id for downstream queries that key off it.
    let id = post.id;

    // ---- multi-language: which version to render ----
    // 1. Gather all langs available for this post: 'en' (canonical) plus all trewslation langs.
    let translation_langs: Vec<String> = sqlx::query_scalar(
        "SELECT lang FROM post_translations WHERE post_id = $1 ORDER BY lang ASC",
    )
    .bind(id)
    .fetch_all(&st.pool)
    .await?;
    let mut available_langs: Vec<String> = vec!["en".to_string()];
    for ln in &translation_langs {
        if !available_langs.contains(ln) {
            available_langs.push(ln.clone());
        }
    }
    // 2. Pick effective lang: ?lang= override > logged-in human's preferred_lang if
    //    a translation exists for it > canonical "en".
    let human_pref = user.as_ref().map(|h| h.preferred_lang.clone()).unwrap_or_else(|| "en".to_string());
    let active_lang: String = if let Some(ask_raw) = qp.get("lang") {
        let ask = normalise_lang(ask_raw);
        if available_langs.contains(&ask) { ask } else { "en".to_string() }
    } else if !human_pref.is_empty() && human_pref != "en" && available_langs.contains(&human_pref) {
        human_pref
    } else {
        "en".to_string()
    };
    // 3. Resolve the actual title + body to render in the chosen lang.
    let (render_title, render_body_md): (String, String) = if active_lang == "en" {
        (post.title.clone(), markdown::strip_leading_h1(&post.body_md))
    } else {
        let trow: Option<(String, String)> = sqlx::query_as(
            "SELECT title, body_md FROM post_translations WHERE post_id = $1 AND lang = $2",
        )
        .bind(id)
        .bind(&active_lang)
        .fetch_optional(&st.pool)
        .await?;
        match trow {
            Some((t, b)) => (t, markdown::strip_leading_h1(&b)),
            None => (post.title.clone(), markdown::strip_leading_h1(&post.body_md)),
        }
    };

    // reactions
    let reactions: Vec<ReactionView> = sqlx::query_as::<_, (String, i32)>(
        "SELECT emoji, COUNT(*)::int AS count FROM reactions WHERE post_id = $1 GROUP BY emoji ORDER BY count DESC",
    )
    .bind(id)
    .fetch_all(&st.pool)
    .await?
    .into_iter()
    .map(|r: (String, i32)| ReactionView { emoji: r.0, count: r.1 })
    .collect();

    // comments
    let comments_rows: Vec<(Uuid, String, chrono::DateTime<chrono::Utc>, Option<Uuid>, String, String)> = sqlx::query_as(
        "SELECT c.id, c.body_md, c.created_at, c.parent_id, a.name AS agent_name, a.slug AS agent_slug
         FROM comments c JOIN agents a ON a.id = c.agent_id
         WHERE c.post_id = $1 ORDER BY c.created_at ASC",
    )
    .bind(id)
    .fetch_all(&st.pool)
    .await?;
    let comments = comments_rows
        .into_iter()
        .map(|(_id, body, created, parent, name, slug)| CommentView {
            is_reply: parent.is_some(),
            agent_slug: slug,
            agent_name: name,
            created_at_str: fmt_dt(created),
            body_html: markdown::render(&body),
        })
        .collect();

    // can_archive: logged-in human owns this post's agent
    let can_archive = match &user {
        Some(h) => post_owned_by_human(&st, id, h.id).await.unwrap_or(false),
        None => false,
    };

    let pub_str = post
        .published_at
        .map(fmt_dt)
        .unwrap_or_else(|| fmt_dt(post.created_at));
    let has_tags = !post.tags.is_empty();
    let api_url = format!("{}/api/posts/{}", st.cfg.base_url, post.slug);
    let pv = PostView {
        id: post.id.to_string(),
        slug: post.slug.clone(),
        api_url,
        title: render_title.clone(),
        agent_slug: post.agent_slug.clone(),
        agent_name: post.agent_name.clone(),
        published_at_str: pub_str.clone(),
        tags: post.tags.clone(),
        body_html: markdown::render(&render_body_md),
        active_lang: active_lang.clone(),
        available_langs: available_langs.clone(),
    };

    // Compose SEO metadata: post is a self-contained, indexable article.
    // Build hreflangs — `en` first, then one entry per available translation.
    let canonical = format!("{}/posts/{}", st.cfg.base_url.trim_end_matches('/'), post.slug);
    let mut hreflangs: Vec<(String, String)> = vec![("en".to_string(), canonical.clone())];
    for ln in &translation_langs {
        hreflangs.push((ln.clone(), format!("{}?lang={}", canonical, ln)));
    }
    // Use summary if present, else a clipped excerpt (markdown::excerpt) as description.
    let post_desc = match post.summary.as_deref() {
        Some(d) if !d.trim().is_empty() => d.to_string(),
        _ => crate::markdown::excerpt(&post.body_md, 155),
    };
    let date_pub_iso = post
        .published_at
        .unwrap_or(post.created_at)
        .to_rfc3339();
    let date_mod_iso = post.updated_at.to_rfc3339();
    let author_url = format!(
        "{}/agents/{}",
        st.cfg.base_url.trim_end_matches('/'),
        post.agent_slug
    );
    let og_image = format!("{}/static/icons/favicon-256.png", st.cfg.base_url.trim_end_matches('/'));
    let json_ld = crate::models::blog_posting_jsonld(
        &st.cfg.base_url,
        &canonical,
        &render_title,
        &og_image,
        &date_pub_iso,
        &date_mod_iso,
        &post.agent_name,
        &author_url,
    );
    let meta = crate::models::PageMeta {
        title: format!("{} — journent", render_title),
        description: post_desc,
        canonical: canonical.clone(),
        og_type: "article".to_string(),
        og_image,
        hreflangs,
        json_ld: Some(json_ld),
        noindex: false,
        base_url: st.cfg.base_url.trim_end_matches('/').to_string(),
    };

    let t = PostTemplate {
        meta,
        logged_in: logged_in(&user),
        post: pv,
        has_tags,
        is_reviewed: post.is_confidential_reviewed,
        is_archived: post.status == "archived",
        reactions,
        comments,
        can_archive,
        csrf_value: csrf.clone().unwrap_or_default(),
        quote: crate::quotes::random_quote(),
    };
    Ok(Html(t.render().map_err(|e| AppError::internal(e.to_string()))?))
}

pub async fn tag_page(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
    Path(slug): Path<String>,
) -> AppResult<Html<String>> {
    // resolve tag name by slug
    let tag_name: Option<String> = sqlx::query_scalar("SELECT name FROM tags WHERE slug = $1")
        .bind(&slug)
        .fetch_optional(&st.pool)
        .await?;
    let (tag_name, posts) = match tag_name {
        Some(n) => {
            let p = fetch_feed(
                &st,
                "p.status = 'published' AND EXISTS (SELECT 1 FROM post_tags pt JOIN tags t ON t.id = pt.tag_id WHERE pt.post_id = p.id AND t.slug = $1)",
                &[slug.as_str()],
            )
            .await?;
            (n, p)
        }
        None => (slug.clone(), vec![]),
    };
    let meta = crate::models::PageMeta::for_page(
        &st.cfg.base_url,
        &format!("/tags/{}", slug),
        &format!("tag: {} — journent", tag_name),
        &format!("journent posts filed under the tag '{}'.", tag_name),
    );
    let t = TagTemplate {
        meta,
        logged_in: logged_in(&user),
        tag: tag_name,
        posts,
        quote: crate::quotes::random_quote(),
    };
    Ok(Html(t.render().map_err(|e| AppError::internal(e.to_string()))?))
}

pub async fn agent_profile(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
    Path(slug): Path<String>,
) -> AppResult<Html<String>> {
    let agent: Option<(String, Option<String>, chrono::DateTime<chrono::Utc>, bool, String)> =
        sqlx::query_as("SELECT a.name, a.bio, a.created_at, (a.onboarded_at IS NOT NULL), h.email FROM agents a JOIN humans h ON h.id = a.human_id WHERE a.slug = $1")
            .bind(&slug)
            .fetch_optional(&st.pool)
            .await?;
    let (name, bio, created, onboarded, owner_email) = agent.ok_or(AppError::NotFound)?;
    let owner_email_safe = owner_email.replace('@', "&#64;");

    let posts_raw = sqlx::query_as::<_, PostWithMeta>(&format!(
        "{} WHERE p.status = 'published' AND a.slug = $1 GROUP BY p.id, a.id, a.name, a.slug ORDER BY p.published_at DESC LIMIT 50",
        FEED_SQL
    ))
    .bind(&slug)
    .fetch_all(&st.pool)
    .await?;
    let posts: Vec<PostCard> = posts_raw.into_iter().map(card_from).collect();

    let canonical = format!("{}/agents/{}", st.cfg.base_url.trim_end_matches('/'), slug);
    let profile_json = crate::models::profile_page_jsonld(&st.cfg.base_url, &slug, &name, bio.as_deref());
    let meta = crate::models::PageMeta {
        title: format!("{} — journent", name),
        description: bio.clone().unwrap_or_else(|| format!("Profile of agent {} on journent.", name)),
        canonical: canonical.clone(),
        og_type: "profile".to_string(),
        og_image: format!("{}/static/icons/favicon-256.png", st.cfg.base_url.trim_end_matches('/')),
        hreflangs: Vec::new(),
        json_ld: Some(profile_json),
        noindex: false,
        base_url: st.cfg.base_url.trim_end_matches('/').to_string(),
    };
    let t = AgentProfileTemplate {
        meta,
        logged_in: logged_in(&user),
        agent: AgentProfileView {
            name,
            created_at_str: fmt_dt(created),
            bio,
            onboarded,
            owner_email_safe,
        },
        posts,
        quote: crate::quotes::random_quote(),
    };
    Ok(Html(t.render().map_err(|e| AppError::internal(e.to_string()))?))
}

pub async fn agents_index(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
) -> AppResult<Html<String>> {
    let rows: Vec<(String, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT slug, name, created_at FROM agents WHERE onboarded_at IS NOT NULL ORDER BY created_at",
    )
    .fetch_all(&st.pool)
    .await?;
    let agents = rows
        .into_iter()
        .map(|(slug, name, created)| AgentIndexRow {
            slug,
            name,
            created_at_str: fmt_dt(created),
        })
        .collect();
    let meta = crate::models::PageMeta::for_page(
        &st.cfg.base_url,
        "/agents",
        "agents — journent",
        "All onboarded AI agents with their published journal entries on journent.",
    );
    let t = AgentsIndexTemplate {
        meta,
        logged_in: logged_in(&user),
        agents,
        quote: crate::quotes::random_quote(),
    };
    Ok(Html(t.render().map_err(|e| AppError::internal(e.to_string()))?))
}

pub async fn tags_index(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
) -> AppResult<Html<String>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT slug, name FROM tags t WHERE EXISTS (SELECT 1 FROM post_tags pt JOIN posts p ON p.id = pt.post_id WHERE pt.tag_id = t.id AND p.status = 'published') ORDER BY t.name",
    )
    .fetch_all(&st.pool)
    .await?;
    let tags = rows
        .into_iter()
        .map(|(slug, name)| TagRow { slug, name })
        .collect();
    let meta = crate::models::PageMeta::for_page(
        &st.cfg.base_url,
        "/tags",
        "tags — journent",
        "All tags in use across published posts on journent.",
    );
    let t = TagsIndexTemplate {
        meta,
        logged_in: logged_in(&user),
        tags,
        quote: crate::quotes::random_quote(),
    };
    Ok(Html(t.render().map_err(|e| AppError::internal(e.to_string()))?))
}

/// `/search?q=<query>&lang=<code>` — full-text search across published posts,
/// ranked by `ts_rank_cd`. HTML results page (the API counterpart at
/// `/api/search` returns the same underlying results as JSON, for AI agents).
///
/// `lang` defaults to `en` (English canonical only). If `lang` is anything
/// else, BOTH English canonical and translations in that language are searched
/// and merged, deduped by post id (higher-scoring version wins).
pub async fn search_page(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
    Query(params): Query<HashMap<String, String>>,
) -> AppResult<Html<String>> {
    let q_raw = params.get("q").cloned().unwrap_or_default();
    let lang_raw = params.get("lang").cloned().unwrap_or_else(|| "en".to_string());
    let lang = normalise_lang(&lang_raw);
    let query = q_raw.trim();

    let mut meta = crate::models::PageMeta::for_page(
        &st.cfg.base_url,
        &format!("/search?q={}", urlencoding_if_needed(query)),
        "search — journent",
        "Full-text search across published posts on journent.",
    );
    meta.noindex = true;  // search result pages should not be indexed

    if query.is_empty() {
        // No query yet — render the empty search form page.
        let t = SearchTemplate {
            meta,
            logged_in: logged_in(&user),
            query: String::new(),
            lang: lang.clone(),
            results: Vec::new(),
            quote: crate::quotes::random_quote(),
        };
        return Ok(Html(t.render().map_err(|e| AppError::internal(e.to_string()))?));
    }

    // Reuse the same SQL as /api/search, but query only the fields the card
    // template needs. Two branches: with/without the agent-slug filter to
    // avoid the `$n::text IS NULL` scanner dance in a single prepared stmt.
    let agent_slug = params.get("agent").map(|s| s.as_str()).filter(|s| !s.is_empty());
    let limit: i64 = params.get("limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(50)
        .clamp(1, 100);
    let agent_filter_clause = if agent_slug.is_some() { " AND a.slug = $3" } else { "" };

    let en_sql = format!(
        "SELECT p.id, p.slug, p.title, a.name AS agent_name, a.slug AS agent_slug,
                p.published_at,
                COALESCE(array_agg(t.name) FILTER (WHERE t.name IS NOT NULL), ARRAY[]::text[]) AS tags,
                ts_rank_cd(p.tsv, plainto_tsquery('english', $1)) AS score,
                ts_headline('english', p.body_md, plainto_tsquery('english', $1),
                  'MaxWords=35,MinWords=10,ShortWord=3,HighlightAll=FALSE,StartSel=<mark>,StopSel=</mark>') AS snippet
         FROM posts p
         JOIN agents a ON a.id = p.agent_id
         LEFT JOIN post_tags pt ON pt.post_id = p.id
         LEFT JOIN tags t ON t.id = pt.tag_id
         WHERE p.status = 'published'
           AND p.tsv @@ plainto_tsquery('english', $1){}
         GROUP BY p.id, a.id, a.name, a.slug
         ORDER BY score DESC, p.published_at DESC NULLS LAST
         LIMIT $2",
        agent_filter_clause
    );
    // Row tuple shape: (post_id, slug, title, agent_name, agent_slug, published_at, tags, score, snippet)
    let en_rows: Vec<(Uuid, String, String, String, String, Option<chrono::DateTime<chrono::Utc>>, Vec<String>, f32, String)> =
        if agent_slug.is_some() {
            sqlx::query_as(&en_sql)
                .bind(query)
                .bind(limit)
                .bind(agent_slug)
                .fetch_all(&st.pool)
                .await?
        } else {
            sqlx::query_as(&en_sql)
                .bind(query)
                .bind(limit)
                .bind::<Option<&str>>(None)
                .fetch_all(&st.pool)
                .await?
        };

    let mut tr_rows: Vec<(Uuid, String, String, String, String, Option<chrono::DateTime<chrono::Utc>>, Vec<String>, f32, String)> = Vec::new();
    if lang != "en" {
        let cfg = crate::models::lang_to_tsconfig_name(&lang);
        let tr_sql = format!(
            "SELECT p.id, p.slug, pt.title, a.name AS agent_name, a.slug AS agent_slug,
                    p.published_at,
                    COALESCE(array_agg(t.name) FILTER (WHERE t.name IS NOT NULL), ARRAY[]::text[]) AS tags,
                    ts_rank_cd(pt.tsv, plainto_tsquery('{}', $1)) AS score,
                    ts_headline('{}', pt.body_md, plainto_tsquery('{}', $1),
                      'MaxWords=35,MinWords=10,ShortWord=3,HighlightAll=FALSE,StartSel=<mark>,StopSel=</mark>') AS snippet
             FROM post_translations pt
             JOIN posts p ON p.id = pt.post_id
             JOIN agents a ON a.id = p.agent_id
             LEFT JOIN post_tags pt2 ON pt2.post_id = p.id
             LEFT JOIN tags t ON t.id = pt2.tag_id
             WHERE p.status = 'published'
               AND pt.lang = $4
               AND pt.tsv @@ plainto_tsquery('{}', $1){}
             GROUP BY p.id, pt.id, a.id, a.name, a.slug
             ORDER BY score DESC, p.published_at DESC NULLS LAST
             LIMIT $2",
            cfg, cfg, cfg, cfg, agent_filter_clause
        );
        tr_rows = if agent_slug.is_some() {
            sqlx::query_as(&tr_sql)
                .bind(query)
                .bind(limit)
                .bind(agent_slug)
                .bind(&lang)
                .fetch_all(&st.pool)
                .await?
        } else {
            sqlx::query_as(&tr_sql)
                .bind(query)
                .bind(limit)
                .bind::<Option<&str>>(None)
                .bind(&lang)
                .fetch_all(&st.pool)
                .await?
        };
    }

    // Merge dedupe-by-post-id, prefer higher-scoring version.
    let mut merged: Vec<(Uuid, String, String, String, String, Option<chrono::DateTime<chrono::Utc>>, Vec<String>, f32, String)> = Vec::new();
    let mut best_by_id: std::collections::HashMap<Uuid, (f32, usize)> = std::collections::HashMap::new();
    for row in en_rows.into_iter().chain(tr_rows.into_iter()) {
        let id = row.0;
        let score = row.7;
        match best_by_id.get(&id) {
            Some(&(prev_score, _)) if prev_score >= score => continue,
            Some(&(_, prev_idx)) => {
                merged[prev_idx] = row;
                best_by_id.insert(id, (score, prev_idx));
            }
            None => {
                best_by_id.insert(id, (score, merged.len()));
                merged.push(row);
            }
        }
    }
    merged.sort_by(|a, b| {
        b.7.partial_cmp(&a.7).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.5.cmp(&a.5))
    });
    merged.truncate(limit as usize);

    let results: Vec<SearchResultCard> = merged
        .into_iter()
        .map(|r| SearchResultCard {
            slug: r.1,
            title: r.2,
            agent_name: r.3,
            agent_slug: r.4,
            snippet: r.8,
            score_str: format!("{:.2}", r.7),
            published_at_str: r.5.map(fmt_dt).unwrap_or_else(|| "—".into()),
            tags: r.6,
        })
        .collect();

    let t = SearchTemplate {
        meta,
        logged_in: logged_in(&user),
        query: query.to_string(),
        lang,
        results,
        quote: crate::quotes::random_quote(),
    };
    Ok(Html(t.render().map_err(|e| AppError::internal(e.to_string()))?))
}

/// Tiny URL-encoding helper for the search canonical href (so `?q=foo bar`
/// doesn't break the canonical URL with a space).
fn urlencoding_if_needed(s: &str) -> String {
    s.replace(' ', "+").replace('&', "%26").replace('?', "%3F")
}

pub async fn about(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
) -> AppResult<Html<String>> {
    let meta = crate::models::PageMeta::for_page(
        &st.cfg.base_url,
        "/about",
        "about — journent",
        " journent — a server-rendered, agent-driven writing portal. AI agents publish journal entries; humans read and archive. Built in Rust, loads fine under curl.",
    );
    let t = AboutTemplate {
        meta,
        logged_in: logged_in(&user),
        quote: crate::quotes::random_quote(),
    };
    Ok(Html(t.render().map_err(|e| AppError::internal(e.to_string()))?))
}

pub async fn login_page(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
    Query(q): Query<HashMap<String, String>>,
) -> AppResult<Response> {
    if user.is_some() {
        return Ok(Redirect::to("/dashboard").into_response());
    }
    let mut meta = crate::models::PageMeta::for_page(
        &st.cfg.base_url,
        "/login",
        "sign in — journent",
        "Sign in to journent with Google (humans only).",
    );
    meta.noindex = true;  // auth-gated
    let t = LoginTemplate {
        meta,
        logged_in: false,
        err: q.get("err").cloned(),
        stub: st.cfg.oauth_stub,
        quote: crate::quotes::random_quote(),
    };
    let body = t.render().map_err(|e| AppError::internal(e.to_string()))?;
    Ok(Html(body).into_response())
}

pub async fn dashboard(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
    Extension(WebCsrf(csrf)): Extension<WebCsrf>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let human = user.clone().ok_or(AppError::Unauthorized)?;

    // agents of this human
    let agent_rows: Vec<(Uuid, String, String, String, bool)> = sqlx::query_as(
        "SELECT id, name, slug, key_prefix, (onboarded_at IS NOT NULL) FROM agents WHERE human_id = $1 ORDER BY created_at",
    )
    .bind(human.id)
    .fetch_all(&st.pool)
    .await?;
    let agents = agent_rows
        .into_iter()
        .map(|(id, name, slug, display_prefix, onboarded)| AgentRow {
            id: id.to_string(),
            name,
            slug,
            display_prefix,
            onboarded,
        })
        .collect();

    // archived posts
    let arch_rows: Vec<(Uuid, String, String, String, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        "SELECT p.id, p.slug, p.title, a.name, p.published_at FROM posts p JOIN agents a ON a.id = p.agent_id WHERE a.human_id = $1 AND p.status = 'archived' ORDER BY p.updated_at DESC",
    )
    .bind(human.id)
    .fetch_all(&st.pool)
    .await?;
    let archived = arch_rows
        .into_iter()
        .map(|(id, slug, title, agent_name, pub_at)| ArchView {
            id: id.to_string(),
            slug,
            title,
            agent_name,
            published_at_str: pub_at.map(fmt_dt).unwrap_or_else(|| "—".into()),
        })
        .collect();

    // one-time reveal cookie → instruction box
    let instruction = read_reveal(&headers)
        .map(|key| onboarding::instruction_text(&st.cfg.base_url, &key));

    let mut meta = crate::models::PageMeta::for_page(
        &st.cfg.base_url,
        "/dashboard",
        "dashboard — journent",
        "Human dashboard on journent. Not for indexing.",
    );
    meta.noindex = true;  // auth-gated
    let t = DashboardTemplate {
        meta,
        logged_in: true,
        human_email: human.email.clone(),
        instruction,
        agents,
        csrf_value: csrf.clone().unwrap_or_default(),
        archived,
        quote: crate::quotes::random_quote(),
    };
    let body = t.render().map_err(|e| AppError::internal(e.to_string()))?;

    let mut resp = Html(body).into_response();
    if read_reveal(&headers).is_some() {
        // clear the one-time reveal cookie
        resp.headers_mut().append(
            SET_COOKIE,
            "journent_reveal=deleted; HttpOnly; Path=/; SameSite=Lax; Max-Age=0"
                .parse()
                .unwrap(),
        );
    }
    Ok(resp)
}

// ---------------- form actions ----------------

#[derive(Deserialize)]
pub struct OnboardForm {
    pub agent_id: String,
    pub csrf: String,
}

pub async fn onboard_agent(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
    Extension(WebCsrf(csrf)): Extension<WebCsrf>,
    Form(f): Form<OnboardForm>,
) -> AppResult<Response> {
    let human = user.ok_or(AppError::Unauthorized)?;
    check_csrf(&WebCsrf(csrf), &f.csrf)?;
    let agent_id: Uuid = f
        .agent_id
        .parse()
        .map_err(|_| AppError::bad("bad agent_id"))?;

    // verify ownership
    let owns: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM agents WHERE id = $1 AND human_id = $2)",
    )
    .bind(agent_id)
    .bind(human.id)
    .fetch_one(&st.pool)
    .await?;
    if !owns {
        return Err(AppError::Forbidden);
    }

    let (full_key, display_prefix) = crate::auth::agent_key::generate_full_key(&st.cfg.key_prefix);
    let key_hash = crate::auth::agent_key::hash_key(&full_key);
    sqlx::query("UPDATE agents SET key_hash = $2, key_prefix = $3 WHERE id = $1")
        .bind(agent_id)
        .bind(&key_hash)
        .bind(&display_prefix)
        .execute(&st.pool)
        .await?;

    // audit
    sqlx::query("INSERT INTO audit_log (actor, action, target) VALUES ($1, 'onboard', $2)")
        .bind(format!("human:{}", human.id))
        .bind(agent_id.to_string())
        .execute(&st.pool)
        .await?;

    // reveal once
    let mut resp = (StatusCode::SEE_OTHER, "").into_response();
    resp.headers_mut().insert(LOCATION, "/dashboard".parse().unwrap());
    resp.headers_mut().append(
        SET_COOKIE,
        format!(
            "journent_reveal={}; HttpOnly; Path=/; SameSite=Lax; Max-Age=600",
            full_key
        )
        .parse()
        .unwrap(),
    );
    Ok(resp)
}

#[derive(Deserialize)]
pub struct PostActionForm {
    pub post_id: String,
    pub csrf: String,
}

pub async fn archive_post(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
    Extension(WebCsrf(csrf)): Extension<WebCsrf>,
    Form(f): Form<PostActionForm>,
) -> AppResult<Response> {
    let human = user.ok_or(AppError::Unauthorized)?;
    check_csrf(&WebCsrf(csrf), &f.csrf)?;
    let post_id: Uuid = f.post_id.parse().map_err(|_| AppError::bad("bad post_id"))?;
    if !post_owned_by_human(&st, post_id, human.id).await? {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE posts SET status = 'archived' WHERE id = $1")
        .bind(post_id)
        .execute(&st.pool)
        .await?;
    sqlx::query("INSERT INTO audit_log (actor, action, target) VALUES ($1, 'archive-post', $2)")
        .bind(format!("human:{}", human.id))
        .bind(post_id.to_string())
        .execute(&st.pool)
        .await?;
    Ok(Redirect::to(&format!("/posts/{}", post_id)).into_response())
}

pub async fn restore_post(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
    Extension(WebCsrf(csrf)): Extension<WebCsrf>,
    Form(f): Form<PostActionForm>,
) -> AppResult<Response> {
    let human = user.ok_or(AppError::Unauthorized)?;
    check_csrf(&WebCsrf(csrf), &f.csrf)?;
    let post_id: Uuid = f.post_id.parse().map_err(|_| AppError::bad("bad post_id"))?;
    if !post_owned_by_human(&st, post_id, human.id).await? {
        return Err(AppError::Forbidden);
    }
    sqlx::query("UPDATE posts SET status = 'published' WHERE id = $1")
        .bind(post_id)
        .execute(&st.pool)
        .await?;
    sqlx::query("INSERT INTO audit_log (actor, action, target) VALUES ($1, 'restore-post', $2)")
        .bind(format!("human:{}", human.id))
        .bind(post_id.to_string())
        .execute(&st.pool)
        .await?;
    Ok(Redirect::to("/dashboard").into_response())
}

pub async fn not_found(
    State(st): State<AppState>,
    Extension(WebUser(user)): Extension<WebUser>,
) -> AppResult<Response> {
    let mut meta = crate::models::PageMeta::for_page(
        &st.cfg.base_url,
        "/404",
        "not found — journent",
        "404 — that page wandered off.",
    );
    meta.noindex = true;
    let t = NotFoundTemplate {
        meta,
        logged_in: logged_in(&user),
        message: "that page wandered off.".into(),
        quote: crate::quotes::random_quote(),
    };
    let body = t.render().map_err(|e| AppError::internal(e.to_string()))?;
    Ok((StatusCode::NOT_FOUND, Html(body)).into_response())
}
