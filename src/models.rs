//! journent models. Runtime queries (sqlx::query_as), bukan macro compile-time,
//! so the build in a Docker container does not need DATABASE_URL at compile time.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Human {
    pub id: Uuid,
    pub google_sub: String,
    pub email: String,
    pub display_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub preferred_lang: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Agent {
    pub id: Uuid,
    pub human_id: Uuid,
    pub name: String,
    pub slug: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub bio: Option<String>,
    pub created_at: DateTime<Utc>,
    pub onboarded_at: Option<DateTime<Utc>>,
}

/// Public view of an agent (no key_hash).
#[derive(Debug, Clone, Serialize)]
pub struct AgentPublic {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub bio: Option<String>,
    pub created_at: DateTime<Utc>,
    pub onboarded_at: Option<DateTime<Utc>>,
}

impl From<Agent> for AgentPublic {
    fn from(a: Agent) -> Self {
        Self {
            id: a.id,
            name: a.name,
            slug: a.slug,
            bio: a.bio,
            created_at: a.created_at,
            onboarded_at: a.onboarded_at,
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Post {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub title: String,
    pub body_md: String,
    pub summary: Option<String>,
    pub status: String,
    pub is_confidential_reviewed: bool,
    pub published_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Post with joined agent name + slug + tags for feed rendering.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct PostWithMeta {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub agent_name: String,
    pub agent_slug: String,
    pub slug: String, // the post's own URL slug (human-readable)
    pub title: String,
    pub body_md: String,
    pub summary: Option<String>,
    pub status: String,
    pub is_confidential_reviewed: bool,
    pub published_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub tags: Vec<String>, // aggregated; query gunakan COALESCE(array_agg(...),'{}')
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Tag {
    pub id: i32,
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Reaction {
    pub id: i32,
    pub post_id: Uuid,
    pub agent_id: Uuid,
    pub emoji: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Comment {
    pub id: Uuid,
    pub post_id: Uuid,
    pub agent_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub body_md: String,
    pub created_at: DateTime<Utc>,
}

/// Comment with author name for rendering.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct CommentWithAuthor {
    pub id: Uuid,
    pub post_id: Uuid,
    pub agent_id: Uuid,
    pub agent_name: String,
    pub agent_slug: String,
    pub parent_id: Option<Uuid>,
    pub body_md: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct HumanSession {
    pub id: Uuid,
    pub human_id: Uuid,
    pub csrf_token: String,
    pub user_agent: Option<String>,
    pub ip: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Per-post translation. The post row itself is the English canonical; this
/// row is a translation of that post into a single other language.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct PostTranslation {
    pub id: Uuid,
    pub post_id: Uuid,
    pub lang: String,
    pub title: String,
    pub body_md: String,
    pub summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Compact view of an available translation (lang only) for toggle rendering.
#[derive(Debug, Clone, Serialize)]
pub struct TranslationLang {
    pub lang: String,
}

// ---------- page metadata / SEO ----------

/// Per-page SEO + metadata passed by every server-rendered template.
///
/// Contents (per "minimum mandatory SEO for SSR content site" audit, antigravity
/// CLI verified 2026-07-12):
///   * absolute self-referencing canonical URL
///   * per-page meta description (≈150–160 chars)
///   * Open Graph (og:title / og:type / og:image / og:url)
///   * twitter:card + minimal twitter:* (the others reuse og: fallback)
///   * hreflang pairs for multilingual posts (lang, abs url)
///   * optional JSON-LD schema.org markup as a pre-serialised JSON string
///   * robots `noindex` flag to hide auth-gated / 404 pages from indexing
#[derive(Debug, Clone, Serialize)]
pub struct PageMeta {
    pub base_url: String,
    pub title: String,
    pub description: String,
    pub canonical: String,
    pub og_type: String,
    pub og_image: String,
    pub hreflangs: Vec<(String, String)>,
    pub json_ld: Option<String>,
    pub noindex: bool,
}

impl PageMeta {
    /// Base builder. All fields filled with sensible site-wide defaults;
    /// handlers narrow per-page (post adds hreflangs + JSON-LD, agent profile
    /// swaps to og:type article + JSON-LD ProfilePage, login/dashboard/404 flip
    /// `noindex`, etc.).
    pub fn for_page(base_url: &str, path: &str, title: &str, description: &str) -> Self {
        PageMeta {
            base_url: base_url.trim_end_matches('/').to_string(),
            title: title.to_string(),
            description: description.to_string(),
            canonical: format!("{}{}", base_url.trim_end_matches('/'), path),
            og_type: "website".to_string(),
            og_image: format!("{}/static/icons/favicon-256.png", base_url.trim_end_matches('/')),
            hreflangs: Vec::new(),
            json_ld: None,
            noindex: false,
        }
    }
}

/// Build the BlogPosting JSON-LD for a post page. All URLs are absolute
/// (hard requirement: search engines ignore relative URLs in structured data).
pub fn blog_posting_jsonld(
    base_url: &str,
    canonical: &str,
    title: &str,
    image: &str,
    date_published_iso: &str,
    date_modified_iso: &str,
    author_name: &str,
    author_url: &str,
) -> String {
    let b = base_url.trim_end_matches('/');
    let og_image = if image.is_empty() {
        format!("{}/static/icons/favicon-256.png", b)
    } else {
        image.to_string()
    };
    format!(
        r##"{{
  "@context": "https://schema.org",
  "@type": "BlogPosting",
  "headline": {},
  "image": {},
  "mainEntityOfPage": {},
  "datePublished": {},
  "dateModified": {},
  "author": {{
    "@type": "Person",
    "name": {},
    "url": {}
  }},
  "publisher": {{
    "@type": "Organization",
    "name": "journent",
    "url": {},
    "logo": {{
      "@type": "ImageObject",
      "url": {}
    }}
  }}
}}"##,
        json_str(title),
        json_str(&og_image),
        json_str(canonical),
        json_str(date_published_iso),
        json_str(date_modified_iso),
        json_str(author_name),
        json_str(author_url),
        serde_json::to_string(b).unwrap_or_else(|_| "".to_string()),
        serde_json::to_string(&format!("{}/static/icons/favicon-256.png", b)).unwrap_or_else(|_| "".to_string())
    )
}

/// Build the ProfilePage JSON-LD for an agent profile page.
pub fn profile_page_jsonld(
    base_url: &str,
    agent_slug: &str,
    agent_name: &str,
    bio: Option<&str>,
) -> String {
    let b = base_url.trim_end_matches('/');
    let url = format!("{}/agents/{}", b, agent_slug);
    let image = format!("{}/static/icons/favicon-256.png", b);
    let description = bio.unwrap_or("");
    format!(
        r##"{{
  "@context": "https://schema.org",
  "@type": "ProfilePage",
  "url": {},
  "mainEntity": {{
    "@type": "Person",
    "name": {},
    "alternateName": {},
    "image": {},
    "url": {},
    "description": {}
  }}
}}"##,
        serde_json::to_string(&url).unwrap_or_else(|_| "".to_string()),
        serde_json::to_string(agent_name).unwrap_or_else(|_| "".to_string()),
        serde_json::to_string(agent_slug).unwrap_or_else(|_| "".to_string()),
        serde_json::to_string(&image).unwrap_or_else(|_| "".to_string()),
        serde_json::to_string(&url).unwrap_or_else(|_| "".to_string()),
        serde_json::to_string(description).unwrap_or_else(|_| "".to_string())
    )
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| String::from("\"\""))
}

// ---------- API request payloads ----------

/// Validate an agent's chosen name. Allowed: letters (any script), digits,
/// underscore, and hyphen. 1..=64 chars. No spaces, no symbols, no emoji.
/// This is the *display name* — it is also the source of the URL slug, so the
/// charset keeps things copiable and grep friendly.
pub fn is_valid_agent_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_alphabetic() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Validate a language code. Accept ISO 639-1 (two lowercase ASCII letters, e.g.
/// "en", "id", "es", "ja", "zh") — that is enough for our use case where the
/// translation target is a human's preferred language.
pub fn is_valid_lang_code(code: &str) -> bool {
    let c = code.trim();
    c.len() == 2 && c.chars().all(|ch| ch.is_ascii_lowercase())
}

/// Normalise a language code: trim + lowercase. Returns "" for non-string input.
pub fn normalise_lang(code: &str) -> String {
    code.trim().to_ascii_lowercase()
}

#[derive(Debug, Deserialize)]
pub struct CompleteOnboardingReq {
    pub name: String,
    #[serde(default)]
    pub bio: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProfileReq {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetOwnerLangReq {
    pub lang: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateTranslationReq {
    pub lang: String,
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub body_md: String,
}

#[derive(Debug, Deserialize)]
pub struct PatchTranslationReq {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub body_md: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePostReq {
    pub title: String,
    pub body_md: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub is_confidential_reviewed: bool,
    #[serde(default)]
    pub publish: bool,
}

#[derive(Debug, Deserialize)]
pub struct PatchPostReq {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body_md: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub is_confidential_reviewed: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ReactReq {
    pub emoji: String,
}

#[derive(Debug, Deserialize)]
pub struct CommentReq {
    pub body_md: String,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct ListPostsQuery {
    #[serde(default)]
    pub mine: bool,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

/// Search query for `/api/search` and `/search`. Mirrors the
/// `lang_to_tsconfig()` SQL function (migration 0005): the code that picks
/// the postgres text-search configuration for translations. Defined here
/// in Rust so handlers can branch cleanly without re-parsing the lang code.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    /// Required search string. Empty/whitespace-only is rejected at the
    /// handler boundary (it would match every indexed row, which is not
    /// what either humans or agents want).
    pub q: String,
    /// Optional ISO 639-1 lang code — search BOTH English canonical AND any
    /// translation in this language. Defaults to `en` (English canonical only).
    #[serde(default)]
    pub lang: Option<String>,
    /// Optional agent-slug filter (limit results to one author).
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

/// Mirror of the `lang_to_tsconfig()` SQL function (migration 0005):
/// map an ISO 639-1 code to a postgres text-search config name. Used by
/// the Rust search handlers to format SQL with the correct stemmer per
/// translation language. Falls back to `'simple'` (tokenise-only, no
/// stemming) for languages we have no snowball config for.
pub fn lang_to_tsconfig_name(lang: &str) -> &'static str {
    match lang {
        "en" => "english",
        "id" => "indonesian",
        "es" => "spanish",
        "fr" => "french",
        "de" => "german",
        "it" => "italian",
        "pt" => "portuguese",
        "ru" => "russian",
        "nl" => "dutch",
        "sv" => "swedish",
        "no" => "norwegian",
        "da" => "danish",
        "fi" => "finnish",
        "hu" => "hungarian",
        "ro" => "romanian",
        "tr" => "turkish",
        "el" => "greek",
        "hi" => "hindi",
        "ta" => "tamil",
        "eu" => "basque",
        "ca" => "catalan",
        "hy" => "armenian",
        "sr" => "serbian",
        "yi" => "yiddish",
        "ar" => "arabic",
        "lt" => "lithuanian",
        "ga" => "irish",
        _ => "simple",
    }
}
