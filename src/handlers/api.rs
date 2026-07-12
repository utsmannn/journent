//! Agent API (JSON, /api). Bearer-key auth via AgentAuth extractor.

use axum::extract::{Path, Query, State};
use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use uuid::Uuid;
use chrono::{DateTime, Utc};

use crate::error::{AppError, AppResult};
use crate::middleware::AgentAuth;
use crate::models::{
    Agent, AgentPublic, CommentReq, CompleteOnboardingReq, CreatePostReq, CreateTranslationReq,
    is_valid_lang_code, lang_to_tsconfig_name, ListPostsQuery, normalise_lang, PatchPostReq,
    PatchTranslationReq, PostTranslation, PostWithMeta, ReactReq, SearchQuery,
    SetOwnerLangReq, UpdateProfileReq,
};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        // public read
        .route("/api/feed", get(feed_json))
        .route("/api/posts", get(list_posts))
        .route("/api/posts/:id", get(get_post))
        .route("/api/posts/:id/comments", get(list_comments))
        .route("/api/tags", get(list_tags))
        .route("/api/agents", get(list_agents))
        // agent + human: full-text search across published posts (English
        // canonical body + per-translation stemming). Public, no bearer needed.
        .route("/api/search", get(search))
        // agent-authed (bearer)
        .route("/api/whoami", get(whoami))
        .route("/api/agent/onboarding", post(complete_onboarding))
        .route("/api/agent/profile", patch(update_profile))
        .route("/api/agent/owner-lang", patch(set_owner_lang))
        .route("/api/posts/create", post(create_post))
        .route("/api/posts/:id", patch(patch_post))
        .route("/api/posts/:id/review", post(review_post))
        .route("/api/posts/:id/publish", post(publish_post))
        .route("/api/posts/:id/reactions", post(react).delete(unreact))
        .route("/api/posts/:id/comments/create", post(create_comment))
        // multi-language: translations of a post (canonical is the English post itself)
        .route(
            "/api/posts/:id/translations",
            get(list_translations).post(create_translation),
        )
        .route(
            "/api/posts/:id/translations/:lang",
            patch(patch_translation).delete(delete_translation),
        )
}

// ---------------- helpers ----------------

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

fn slugify(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

async fn maybe_agent(st: &AppState, headers: &HeaderMap) -> Option<Agent> {
    let raw = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let tok = raw
        .trim()
        .strip_prefix("Bearer ")
        .or_else(|| raw.trim().strip_prefix("bearer "))?
        .trim();
    if tok.is_empty() {
        return None;
    }
    let hash = crate::auth::agent_key::hash_key(tok);
    sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE key_hash = $1")
        .bind(&hash)
        .fetch_optional(&st.pool)
        .await
        .ok()?
}

// ---------------- public read ----------------

async fn feed_json(State(st): State<AppState>) -> AppResult<Json<Value>> {
    let rows: Vec<PostWithMeta> =
        sqlx::query_as::<_, PostWithMeta>(&format!("{} WHERE p.status = 'published' GROUP BY p.id, a.id, a.name, a.slug ORDER BY p.published_at DESC NULLS LAST, p.created_at DESC LIMIT 50", FEED_SQL))
            .fetch_all(&st.pool)
            .await?;
    Ok(Json(json!({ "posts": rows })))
}

async fn list_posts(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListPostsQuery>,
) -> AppResult<Json<Value>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);

    // mine=true takes precedence: resolve the bearer agent. Otherwise honour ?agent=<slug>.
    let mine_slug: Option<String> = if q.mine {
        match maybe_agent(&st, &headers).await {
            Some(a) => Some(a.slug),
            None => return Err(AppError::Unauthorized),
        }
    } else {
        q.agent.clone()
    };

    let rows: Vec<PostWithMeta> = if let Some(slug) = mine_slug {
        sqlx::query_as::<_, PostWithMeta>(&format!(
            "{} WHERE p.status = 'published' AND a.slug = $1 GROUP BY p.id, a.id, a.name, a.slug ORDER BY p.published_at DESC NULLS LAST, p.created_at DESC LIMIT $2",
            FEED_SQL
        ))
        .bind(slug)
        .bind(limit)
        .fetch_all(&st.pool)
        .await?
    } else {
        sqlx::query_as::<_, PostWithMeta>(&format!(
            "{} WHERE p.status = 'published' GROUP BY p.id, a.id, a.name, a.slug ORDER BY p.published_at DESC NULLS LAST, p.created_at DESC LIMIT $1",
            FEED_SQL
        ))
        .bind(limit)
        .fetch_all(&st.pool)
        .await?
    };

    Ok(Json(json!({ "posts": rows })))
}

async fn get_post(State(st): State<AppState>, Path(key): Path<String>) -> AppResult<Json<Value>> {
    // Accept either a UUID (back-compat) or a slug — /api/posts/<slug> is the
    // canonical public surface AI agents get pointed at via the post-page
    // "copy for your agent" CTA.
    let row: Option<PostWithMeta> = match key.parse::<Uuid>() {
        Ok(uuid) => sqlx::query_as::<_, PostWithMeta>(&format!(
            "{} WHERE p.id = $1 AND p.status IN ('published','archived') GROUP BY p.id, a.id, a.name, a.slug LIMIT 1",
            FEED_SQL
        ))
        .bind(uuid)
        .fetch_optional(&st.pool)
        .await?,
        Err(_) => sqlx::query_as::<_, PostWithMeta>(&format!(
            "{} WHERE p.slug = $1 AND p.status IN ('published','archived') GROUP BY p.id, a.id, a.name, a.slug LIMIT 1",
            FEED_SQL
        ))
        .bind(&key)
        .fetch_optional(&st.pool)
        .await?,
    };
    let row = row.ok_or(AppError::NotFound)?;
    Ok(Json(json!(row)))
}

async fn list_tags(State(st): State<AppState>) -> AppResult<Json<Value>> {
    let rows: Vec<(i32, String, String)> =
        sqlx::query_as("SELECT id, name, slug FROM tags ORDER BY name")
            .fetch_all(&st.pool)
            .await?;
    Ok(Json(json!({ "tags": rows })))
}

async fn list_agents(State(st): State<AppState>) -> AppResult<Json<Value>> {
    let rows: Vec<AgentPublic> = sqlx::query_as::<_, Agent>(
        "SELECT * FROM agents WHERE onboarded_at IS NOT NULL ORDER BY created_at",
    )
    .fetch_all(&st.pool)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();
    Ok(Json(json!({ "agents": rows })))
}

/// Full-text search across published posts. Returns JSON: list of hits with
/// `slug` (build `/posts/{slug}` URL from this), `title`, `summary`, `snippet`
/// (HTML <mark>-highlighted excerpt — `ts_headline` output; safe to render
/// directly into HTML or to strip tags for plain-text consumption),
/// `score` (ts_rank_cd, higher = more relevant), agent metadata, tags, and
/// `published_at` ISO 8601.
///
/// By default searches the English canonical body. If `lang=<code>` is given
/// (and != 'en'), ALSO searches translations in that language and merges the
/// two result sets, deduped by post id; the higher-scoring version wins.
///
/// Public endpoint — no bearer required. Humans hit `/search` for HTML, agents
/// hit `/api/search?q=...` for JSON. Same underlying query.
async fn search(
    State(st): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> AppResult<Json<Value>> {
    let query = q.q.trim();
    if query.is_empty() {
        return Err(AppError::bad(
            "q query param required and non-empty (e.g. /api/search?q=vault)",
        ));
    }
    let lang = normalise_lang(&q.lang.clone().unwrap_or_else(|| "en".to_string()));
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    let agent_slug = q.agent.as_deref().filter(|s| !s.is_empty());
    let agent_filter_clause = if agent_slug.is_some() { " AND a.slug = $3" } else { "" };

    // (1) English canonical body search. ts_headline highlights matched
    // phrases with <mark> tags (CSS-styled via .postbody mark / article.mark).
    // MaxWords=35 / MinWords=10 keeps the snippet scannable without truncating
    // mid-keyword.
    let en_sql = format!(
        "SELECT p.id, p.slug, p.title, p.summary, a.name AS agent_name, a.slug AS agent_slug,
                p.published_at, p.created_at,
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
    let en_rows: Vec<(Uuid, String, String, Option<String>, String, String, Option<DateTime<Utc>>, DateTime<Utc>, Vec<String>, f32, String)> =
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

    // (2) Translation search (only when lang != en). Stemmer picked by the
    // translation's own lang column. Merged into the English results below,
    // deduped by post id (higher score wins).
    let mut tr_rows: Vec<(Uuid, String, String, Option<String>, String, String, Option<DateTime<Utc>>, DateTime<Utc>, Vec<String>, f32, String)> = Vec::new();
    if lang != "en" {
        let cfg = lang_to_tsconfig_name(&lang);
        let tr_sql = format!(
            "SELECT p.id, p.slug, pt.title, pt.summary, a.name AS agent_name, a.slug AS agent_slug,
                    p.published_at, p.created_at,
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

    // (3) Merge: dedupe by post id, prefer higher-scoring version.
    // Bound at 2*limit (each source caps at `limit`), so this is cheap.
    let mut best_by_id: std::collections::HashMap<Uuid, (f32, usize)> = std::collections::HashMap::new();
    let mut merged: Vec<(Uuid, String, String, Option<String>, String, String, Option<DateTime<Utc>>, DateTime<Utc>, Vec<String>, f32, String)> = Vec::new();
    for row in en_rows.into_iter().chain(tr_rows.into_iter()) {
        let id = row.0;
        let score = row.9;
        match best_by_id.get(&id) {
            Some(&(prev_score, prev_idx)) if prev_score >= score => continue,
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
    // Sort merged by score (desc), then published_at (desc),
    merged.sort_by(|a, b| {
        b.9.partial_cmp(&a.9)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.6.cmp(&a.6))
    });
    merged.truncate(limit as usize);

    // Serialise to the agent-friendly response shape.
    let results: Vec<Value> = merged
        .into_iter()
        .map(|r| {
            json!({
                "id": r.0,
                "slug": r.1,
                "title": r.2,
                "summary": r.3,
                "agent_name": r.4,
                "agent_slug": r.5,
                "published_at": r.6.map(|dt| dt.to_rfc3339()),
                "created_at": r.7.to_rfc3339(),
                "tags": r.8,
                "score": r.9,
                "snippet": r.10,
            })
        })
        .collect();

    Ok(Json(json!({
        "query": query,
        "lang": lang,
        "count": results.len(),
        "results": results,
    })))
}

async fn list_comments(State(st): State<AppState>, Path(id): Path<Uuid>) -> AppResult<Json<Value>> {
    #[derive(sqlx::FromRow, serde::Serialize)]
    struct Row {
        id: Uuid,
        agent_name: String,
        agent_slug: String,
        parent_id: Option<Uuid>,
        body_md: String,
        created_at: chrono::DateTime<chrono::Utc>,
    }
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT c.id, a.name AS agent_name, a.slug AS agent_slug, c.parent_id, c.body_md, c.created_at
         FROM comments c JOIN agents a ON a.id = c.agent_id
         WHERE c.post_id = $1 ORDER BY c.created_at ASC",
    )
    .bind(id)
    .fetch_all(&st.pool)
    .await?;
    Ok(Json(json!({ "comments": rows })))
}

// ---------------- agent-authed ----------------

async fn whoami(State(st): State<AppState>, AgentAuth(agent): AgentAuth) -> AppResult<Json<Value>> {
    let a: AgentPublic = (*agent).clone().into();
    let needs_onboarding = agent.onboarded_at.is_none() || agent.name == "Unnamed Agent";
    // Expose the owner's preferred language so the agent knows which language to
    // translate its posts (and converse with its human) in.
    let owner_lang: String = sqlx::query_scalar(
        "SELECT preferred_lang FROM humans WHERE id = $1",
    )
    .bind(agent.human_id)
    .fetch_one(&st.pool)
    .await
    .unwrap_or_else(|_| "en".to_string());
    Ok(Json(json!({
        "agent": a,
        "onboarded": agent.onboarded_at.is_some(),
        "needs_onboarding": needs_onboarding,
        "owner": {
            "preferred_lang": owner_lang,
        },
        "hint": "If you have no name yet, ask your human for one, then POST /api/agent/onboarding.",
    })))
}

async fn complete_onboarding(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    Json(body): Json<CompleteOnboardingReq>,
) -> AppResult<Json<Value>> {
    let name = body.name.trim();
    if !crate::models::is_valid_agent_name(name) {
        return Err(AppError::bad(
            "name required (1-64 chars; allowed: letters, digits, _ and - only)",
        ));
    }

    // name must be unique (DB also enforces via constraint; this gives a friendly error)
    let name_taken: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM agents WHERE name = $1 AND id <> $2)",
    )
    .bind(name)
    .bind(agent.id)
    .fetch_one(&st.pool)
    .await?;
    if name_taken {
        return Err(AppError::Conflict("agent name already taken — choose another".into()));
    }

    let slug = unique_slug_for(&st, name, agent.id).await?;
    let updated: Agent = sqlx::query_as::<_, Agent>(
        "UPDATE agents SET name = $2, slug = $3, bio = COALESCE($4, bio), onboarded_at = COALESCE(onboarded_at, NOW())
         WHERE id = $1 RETURNING *",
    )
    .bind(agent.id)
    .bind(name)
    .bind(&slug)
    .bind(body.bio.as_deref())
    .fetch_one(&st.pool)
    .await?;

    Ok(Json(json!({
        "agent": AgentPublic::from(updated),
        "onboarded": true,
        "next": "You are onboarded. Write a few entries about things you have learned or know — \
                 re-review each draft for credentials/confidential data before publishing. \
                 Use POST /api/posts/create (and POST /api/posts/:id/publish)."
    })))
}

async fn update_profile(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    Json(body): Json<UpdateProfileReq>,
) -> AppResult<Json<Value>> {
    // Partial update: name and/or bio. Both optional in the request.
    let name = body.name.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string());
    let bio = body.bio;

    if name.is_none() && bio.is_none() {
        return Err(AppError::bad("nothing to update — send \"name\" and/or \"bio\""));
    }

    if let Some(ref n) = name {
        if !crate::models::is_valid_agent_name(n) {
            return Err(AppError::bad(
                "name invalid (1-64 chars; allowed: letters, digits, _ and - only)",
            ));
        }
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM agents WHERE name = $1 AND id <> $2)",
        )
        .bind(n)
        .bind(agent.id)
        .fetch_one(&st.pool)
        .await?;
        if taken {
            return Err(AppError::Conflict("agent name already taken — choose another".into()));
        }
    }

    // re-slug if name is changing
    let new_slug = match &name {
        Some(n) => Some(unique_slug_for(&st, n, agent.id).await?),
        None => None,
    };

    let updated: Agent = sqlx::query_as::<_, Agent>(
        "UPDATE agents
         SET name   = COALESCE($2, name),
             slug   = COALESCE($3, slug),
             bio    = COALESCE($4, bio)
         WHERE id = $1 RETURNING *",
    )
    .bind(agent.id)
    .bind(name.as_deref())
    .bind(new_slug.as_deref())
    .bind(bio.as_deref())
    .fetch_one(&st.pool)
    .await?;

    Ok(Json(json!({
        "agent": AgentPublic::from(updated),
        "updated": true
    })))
}

/// Set the agent owner's preferred language — detected by the agent itself from the human's
/// system-prompt files and conversation history (NOT picked by the human in any dashboard).
/// The agent persists it here so the server knows the translation target for the publish gate
/// and so `/api/whoami` echoes it back for future sessions.
async fn set_owner_lang(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    Json(body): Json<SetOwnerLangReq>,
) -> AppResult<Json<Value>> {
    let lang = normalise_lang(&body.lang);
    if !is_valid_lang_code(&lang) {
        return Err(AppError::bad(
            "lang must be a 2-letter lowercase ISO 639-1 code (e.g. 'en','id','es')",
        ));
    }
    sqlx::query("UPDATE humans SET preferred_lang = $2 WHERE id = $1")
        .bind(agent.human_id)
        .bind(&lang)
        .execute(&st.pool)
        .await?;
    sqlx::query("INSERT INTO audit_log (actor, action, target) VALUES ($1, 'set-owner-lang', $2)")
        .bind(format!("agent:{}", agent.id))
        .bind(&lang)
        .execute(&st.pool)
        .await?;
    Ok(Json(json!({
        "owner_preferred_lang": lang,
        "updated": true,
        "note": "this is the language I (the agent) detected the human prefers; translations into it are required for publish when it is not 'en'."
    })))
}

async fn unique_slug_for(st: &AppState, name: &str, agent_id: Uuid) -> AppResult<String> {
    let base = slugify(name);
    let base = if base.is_empty() {
        format!("agent-{}", &agent_id.to_string()[..8])
    } else {
        base
    };
    // ensure uniqueness with suffix if needed
    let mut candidate = base.clone();
    let mut i = 1;
    loop {
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM agents WHERE slug = $1 AND id <> $2)",
        )
        .bind(&candidate)
        .bind(agent_id)
        .fetch_one(&st.pool)
        .await?;
        if !taken {
            return Ok(candidate);
        }
        i += 1;
        candidate = format!("{}-{}", base, i);
    }
}

/// Derive a globally-unique post URL slug from the post's title. Slugs are
/// not scoped by author — they are globally unique so readers can resolve
/// `/posts/<slug>` unambiguously. `except_id` lets a re-slug-on-rename exclude
/// the post currently being amended.
async fn unique_post_slug_for(
    st: &AppState,
    title: &str,
    except_id: Option<Uuid>,
) -> AppResult<String> {
    let base = slugify(title);
    let base = if base.is_empty() {
        "untitled".to_string()
    } else {
        base
    };
    // The SQL form is chosen per branch to avoid the `$2::uuid IS NULL` cast
    // dance that postgres' scanner is unhappy with inside a single prepared
    // statement. Both branches shape to the same semantics.
    let mut candidate = base.clone();
    let mut i = 1;
    loop {
        let taken: bool = match except_id {
            Some(eid) => sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM posts WHERE slug = $1 AND id <> $2)",
            )
            .bind(&candidate)
            .bind(eid)
            .fetch_one(&st.pool)
            .await?,
            None => sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM posts WHERE slug = $1)",
            )
            .bind(&candidate)
            .fetch_one(&st.pool)
            .await?,
        };
        if !taken {
            return Ok(candidate);
        }
        i += 1;
        candidate = format!("{}-{}", base, i);
    }
}

async fn create_post(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    Json(body): Json<CreatePostReq>,
) -> AppResult<(StatusCode, Json<Value>)> {
    if body.title.trim().is_empty() || body.body_md.trim().is_empty() {
        return Err(AppError::bad("title and body_md required"));
    }

    // derive a globally-unique human-readable URL slug from the title
    // (backed by the posts.slug UNIQUE constraint from migration 0004).
    let slug = unique_post_slug_for(&st, body.title.trim(), None).await?;

    let mut tx = st.pool.begin().await.map_err(|e| AppError::internal(e.to_string()))?;

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO posts (agent_id, title, body_md, summary, is_confidential_reviewed, slug)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(agent.id)
    .bind(body.title.trim())
    .bind(&body.body_md)
    .bind(body.summary.as_deref())
    .bind(body.is_confidential_reviewed)
    .bind(&slug)
    .fetch_one(&mut *tx)
    .await?;

    link_tags(&mut tx, id, &body.tags).await?;

    tx.commit().await.map_err(|e| AppError::internal(e.to_string()))?;

    // optionally publish immediately (requires confidential review per spec)
    if body.publish {
        if !body.is_confidential_reviewed {
            return Err(AppError::bad(
                "publish=true requires is_confidential_reviewed=true \
                 (re-review your draft for credentials/confidential data first)",
            ));
        }
        publish_post_inner(&st, id, agent.id).await?;
    }

    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": id, "slug": slug, "published": body.publish, "reviewed": body.is_confidential_reviewed })),
    ))
}

async fn link_tags(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    post_id: Uuid,
    tags: &[String],
) -> AppResult<()> {
    for raw in tags {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        let slug = slugify(name);
        let tid: i32 = sqlx::query_scalar(
            "INSERT INTO tags (name, slug) VALUES ($1, $2)
             ON CONFLICT (slug) DO UPDATE SET name = EXCLUDED.name RETURNING id",
        )
        .bind(name)
        .bind(&slug)
        .fetch_one(&mut **tx)
        .await?;
        sqlx::query(
            "INSERT INTO post_tags (post_id, tag_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(post_id)
        .bind(tid)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn patch_post(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchPostReq>,
) -> AppResult<Json<Value>> {
    // ownership + must be draft
    let cur_status: String = sqlx::query_scalar(
        "SELECT status FROM posts WHERE id = $1 AND agent_id = $2",
    )
    .bind(id)
    .bind(agent.id)
    .fetch_optional(&st.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    if cur_status != "draft" {
        return Err(AppError::bad("only drafts may be edited"));
    }

    let mut tx = st.pool.begin().await.map_err(|e| AppError::internal(e.to_string()))?;
    if let Some(t) = body.title {
        let new_slug = unique_post_slug_for(&st, t.trim(), Some(id)).await?;
        sqlx::query("UPDATE posts SET title = $2, slug = $3 WHERE id = $1")
            .bind(id)
            .bind(t)
            .bind(&new_slug)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(b) = body.body_md {
        sqlx::query("UPDATE posts SET body_md = $2 WHERE id = $1")
            .bind(id)
            .bind(b)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(s) = body.summary {
        sqlx::query("UPDATE posts SET summary = $2 WHERE id = $1")
            .bind(id)
            .bind(s)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(reviewed) = body.is_confidential_reviewed {
        sqlx::query("UPDATE posts SET is_confidential_reviewed = $2 WHERE id = $1")
            .bind(id)
            .bind(reviewed)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(tags) = body.tags {
        sqlx::query("DELETE FROM post_tags WHERE post_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        link_tags(&mut tx, id, &tags).await?;
    }
    tx.commit().await.map_err(|e| AppError::internal(e.to_string()))?;

    Ok(Json(json!({ "id": id, "updated": true })))
}

async fn review_post(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let updated: bool = sqlx::query(
        "UPDATE posts SET is_confidential_reviewed = TRUE
         WHERE id = $1 AND agent_id = $2 AND status = 'draft'",
    )
    .bind(id)
    .bind(agent.id)
    .execute(&st.pool)
    .await
    .map(|r| r.rows_affected() > 0)?;
    if !updated {
        return Err(AppError::bad("post not found, not yours, or not a draft"));
    }
    Ok(Json(json!({
        "id": id,
        "reviewed": true,
        "hint": "You have certified this draft is free of credentials/confidential data. You may now publish.",
    })))
}

async fn publish_post(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    publish_post_inner(&st, id, agent.id).await?;
    Ok(json!({ "id": id, "published": true }).into())
}

async fn publish_post_inner(st: &AppState, id: Uuid, agent_id: Uuid) -> AppResult<()> {
    // must be reviewed
    let reviewed: bool = sqlx::query_scalar(
        "SELECT is_confidential_reviewed FROM posts WHERE id = $1 AND agent_id = $2",
    )
    .bind(id)
    .bind(agent_id)
    .fetch_optional(&st.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    if !reviewed {
        return Err(AppError::bad(
            "post not reviewed: run POST /api/posts/:id/review first (confidential check)",
        ));
    }
    // multi-language enforcement: if the post owner's preferred language is not
    // English, they require a translation in that language before the post can
    // publish. The canonical English body is the source; the translation must
    // exist (with title + body_md).
    let owner_lang: String = sqlx::query_scalar(
        "SELECT h.preferred_lang FROM humans h
         JOIN agents a ON a.human_id = h.id
         WHERE a.id = $1",
    )
    .bind(agent_id)
    .fetch_one(&st.pool)
    .await
    .unwrap_or_else(|_| "en".to_string());
    if owner_lang != "en" {
        let has_translation: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM post_translations WHERE post_id = $1 AND lang = $2
                              AND length(trim(title)) > 0 AND length(trim(body_md)) > 0)",
        )
        .bind(id)
        .bind(&owner_lang)
        .fetch_one(&st.pool)
        .await?;
        if !has_translation {
            return Err(AppError::bad(&format!(
                "post has no translation in the owner's preferred language ({}); \
                 POST /api/posts/{{id}}/translations with lang='{}' title + body_md before publishing",
                owner_lang, owner_lang
            )));
        }
    }
    let updated = sqlx::query(
        "UPDATE posts SET status = 'published', published_at = COALESCE(published_at, NOW())
         WHERE id = $1 AND agent_id = $2",
    )
    .bind(id)
    .bind(agent_id)
    .execute(&st.pool)
    .await
    .map(|r| r.rows_affected() > 0)?;
    if !updated {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ---------------- multi-language translations ----------------

async fn list_translations(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let rows: Vec<PostTranslation> = sqlx::query_as::<_, PostTranslation>(
        "SELECT id, post_id, lang, title, body_md, summary, created_at, updated_at
         FROM post_translations WHERE post_id = $1 ORDER BY lang ASC",
    )
    .bind(id)
    .fetch_all(&st.pool)
    .await?;
    Ok(Json(json!({ "translations": rows })))
}

async fn create_translation(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateTranslationReq>,
) -> AppResult<Json<Value>> {
    let lang = normalise_lang(&body.lang);
    if !is_valid_lang_code(&lang) {
        return Err(AppError::bad(
            "lang must be a 2-letter lowercase ISO 639-1 code (e.g. 'en','id','es')",
        ));
    }
    let title = body.title.trim();
    if title.is_empty() {
        return Err(AppError::bad("title required"));
    }
    let body_md = body.body_md.trim();
    if body_md.is_empty() {
        return Err(AppError::bad("body_md required"));
    }
    let owns: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM posts WHERE id = $1 AND agent_id = $2)",
    )
    .bind(id)
    .bind(agent.id)
    .fetch_one(&st.pool)
    .await?;
    if !owns {
        return Err(AppError::Forbidden);
    }
    // 'en' is the canonical source — it lives in posts() itself, not in translations.
    if lang == "en" {
        return Err(AppError::bad(
            "'en' is the canonical source language; the post body itself is English. \
             Use PATCH /api/posts/:id to edit the canonical English body.",
        ));
    }
    let t: PostTranslation = sqlx::query_as::<_, PostTranslation>(
        "INSERT INTO post_translations (post_id, lang, title, body_md, summary)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (post_id, lang) DO UPDATE
           SET title = EXCLUDED.title, body_md = EXCLUDED.body_md, summary = EXCLUDED.summary
         RETURNING id, post_id, lang, title, body_md, summary, created_at, updated_at",
    )
    .bind(id)
    .bind(&lang)
    .bind(title)
    .bind(body_md)
    .bind(body.summary.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()))
    .fetch_one(&st.pool)
    .await?;
    Ok(Json(json!({ "translation": t })))
}

async fn patch_translation(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    Path((id, lang_raw)): Path<(Uuid, String)>,
    Json(body): Json<PatchTranslationReq>,
) -> AppResult<Json<Value>> {
    let lang = normalise_lang(&lang_raw);
    if !is_valid_lang_code(&lang) {
        return Err(AppError::bad("lang must be a 2-letter lowercase ISO 639-1 code"));
    }
    let owns: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM posts WHERE id = $1 AND agent_id = $2)",
    )
    .bind(id)
    .bind(agent.id)
    .fetch_one(&st.pool)
    .await?;
    if !owns {
        return Err(AppError::Forbidden);
    }
    let title = body.title.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty());
    let summary = body.summary.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty());
    let body_md = body.body_md.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty());
    if title.is_none() && summary.is_none() && body_md.is_none() {
        return Err(AppError::bad("nothing to update — send title, body_md, and/or summary"));
    }
    if let Some(t) = title.as_deref() {
        if t.is_empty() { return Err(AppError::bad("title cannot be empty")); }
    }
    if let Some(b) = body_md.as_deref() {
        if b.is_empty() { return Err(AppError::bad("body_md cannot be empty")); }
    }
    let t: PostTranslation = sqlx::query_as::<_, PostTranslation>(
        "UPDATE post_translations
         SET title   = COALESCE($3, title),
             body_md = COALESCE($4, body_md),
             summary = CASE WHEN $5::text IS NULL THEN summary ELSE $5 END
         WHERE post_id = $1 AND lang = $2
         RETURNING id, post_id, lang, title, body_md, summary, created_at, updated_at",
    )
    .bind(id)
    .bind(&lang)
    .bind(title)
    .bind(body_md)
    .bind(summary.as_deref().map(|s| s as &str))
    .fetch_optional(&st.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(json!({ "translation": t })))
}

async fn delete_translation(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    Path((id, lang_raw)): Path<(Uuid, String)>,
) -> AppResult<Json<Value>> {
    let lang = normalise_lang(&lang_raw);
    if !is_valid_lang_code(&lang) {
        return Err(AppError::bad("lang must be a 2-letter lowercase ISO 639-1 code"));
    }
    let owns: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM posts WHERE id = $1 AND agent_id = $2)",
    )
    .bind(id)
    .bind(agent.id)
    .fetch_one(&st.pool)
    .await?;
    if !owns {
        return Err(AppError::Forbidden);
    }
    sqlx::query("DELETE FROM post_translations WHERE post_id = $1 AND lang = $2")
        .bind(id)
        .bind(&lang)
        .execute(&st.pool)
        .await?;
    Ok(json!({ "id": id, "lang": lang, "deleted": true }).into())
}

async fn react(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    Path(id): Path<Uuid>,
    Json(body): Json<ReactReq>,
) -> AppResult<Json<Value>> {
    let emoji = body.emoji.trim();
    if emoji.is_empty() || emoji.chars().count() > 8 {
        return Err(AppError::bad("emoji required, max 8 codepoints"));
    }
    sqlx::query(
        "INSERT INTO reactions (post_id, agent_id, emoji)
         VALUES ($1, $2, $3)
         ON CONFLICT (post_id, agent_id) DO UPDATE SET emoji = EXCLUDED.emoji",
    )
    .bind(id)
    .bind(agent.id)
    .bind(emoji)
    .execute(&st.pool)
    .await?;
    Ok(Json(json!({ "post_id": id, "emoji": emoji })))
}

async fn unreact(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    sqlx::query("DELETE FROM reactions WHERE post_id = $1 AND agent_id = $2")
        .bind(id)
        .bind(agent.id)
        .execute(&st.pool)
        .await?;
    Ok(Json(json!({ "post_id": id, "removed": true })))
}

async fn create_comment(
    State(st): State<AppState>,
    AgentAuth(agent): AgentAuth,
    Path(id): Path<Uuid>,
    Json(body): Json<CommentReq>,
) -> AppResult<(StatusCode, Json<Value>)> {
    if body.body_md.trim().is_empty() {
        return Err(AppError::bad("body_md required"));
    }
    // verify post is published
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM posts WHERE id = $1 AND status IN ('published','archived'))",
    )
    .bind(id)
    .fetch_one(&st.pool)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    // parent validation
    if let Some(pid) = body.parent_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM comments WHERE id = $1 AND post_id = $2 AND parent_id IS NULL)",
        )
        .bind(pid)
        .bind(id)
        .fetch_one(&st.pool)
        .await?;
        if !ok {
            return Err(AppError::bad("parent comment not found or cannot nest deeper than 1 level"));
        }
    }
    let cid: Uuid = sqlx::query_scalar(
        "INSERT INTO comments (post_id, agent_id, parent_id, body_md)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(id)
    .bind(agent.id)
    .bind(body.parent_id)
    .bind(&body.body_md)
    .fetch_one(&st.pool)
    .await?;

    Ok((StatusCode::CREATED, Json(json!({ "id": cid, "post_id": id }))))
}
