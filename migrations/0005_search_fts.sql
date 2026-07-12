-- 0005_search_fts.sql
-- postgres full-text search across posts (English canonical body) and the
-- per-language translations. Stemmed tokenisation + GIN index + querying via
-- `plainto_tsquery` (operator-escaped, safe for arbitrary human / agent
-- string). Ranking via `ts_rank_cd`; snippets via `ts_headline` (<mark>
-- wrappers, styled via the existing <mark> CSS).

-- Map ISO 639-1 lang code -> postgres text-search configuration name. Falls
-- back to 'simple' (tokenise, no stemming) for languages we have no snowball
-- config for. IMMUTABLE so it may appear in a generated column expression,
-- and is also callable from the trigger below.
CREATE OR REPLACE FUNCTION lang_to_tsconfig(lang text) RETURNS text
LANGUAGE sql IMMUTABLE STRICT
AS $$
  SELECT CASE lang
    WHEN 'en' THEN 'english'
    WHEN 'id' THEN 'indonesian'
    WHEN 'es' THEN 'spanish'
    WHEN 'fr' THEN 'french'
    WHEN 'de' THEN 'german'
    WHEN 'it' THEN 'italian'
    WHEN 'pt' THEN 'portuguese'
    WHEN 'ru' THEN 'russian'
    WHEN 'nl' THEN 'dutch'
    WHEN 'sv' THEN 'swedish'
    WHEN 'no' THEN 'norwegian'
    WHEN 'da' THEN 'danish'
    WHEN 'fi' THEN 'finnish'
    WHEN 'hu' THEN 'hungarian'
    WHEN 'ro' THEN 'romanian'
    WHEN 'tr' THEN 'turkish'
    WHEN 'el' THEN 'greek'
    WHEN 'hi' THEN 'hindi'
    WHEN 'ta' THEN 'tamil'
    WHEN 'eu' THEN 'basque'
    WHEN 'ca' THEN 'catalan'
    WHEN 'hy' THEN 'armenian'
    WHEN 'sr' THEN 'serbian'
    WHEN 'yi' THEN 'yiddish'
    WHEN 'ar' THEN 'arabic'
    WHEN 'lt' THEN 'lithuanian'
    WHEN 'ga' THEN 'irish'
    ELSE 'simple'
  END
$$;

-- posts.tsv: combined English-canonical tsvector (title weight A heaviest,
-- summary weight B, body weight C). STORED GENERATED column auto-updates on
-- INSERT / UPDATE / DELETE — no trigger to maintain. The 'english' config
-- literal is immutable, so this column is allowed.
ALTER TABLE posts ADD COLUMN tsv tsvector
  GENERATED ALWAYS AS (
    setweight(to_tsvector('english', coalesce(title,   '')), 'A') ||
    setweight(to_tsvector('english', coalesce(summary, '')), 'B') ||
    setweight(to_tsvector('english', coalesce(body_md, '')), 'C')
  ) STORED;

CREATE INDEX idx_posts_tsv ON posts USING GIN (tsv);

-- post_translations: per-translation stemmer picked from the row's own lang
-- column. A GENERATED column can't call `to_tsvector(<runtime lang>::regconfig,
-- …)` because the regconfig cast from text is not immutable by postgres'
-- rules — so we use a plain tsvector column + a trigger that recomputes it
-- on INSERT / UPDATE using lang_to_tsconfig() at write time. Same effect,
-- GIN-indexable.
ALTER TABLE post_translations ADD COLUMN tsv tsvector;

CREATE INDEX idx_post_translations_tsv ON post_translations USING GIN (tsv);

CREATE OR REPLACE FUNCTION post_translations_tsv_update() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  NEW.tsv :=
    setweight(to_tsvector(lang_to_tsconfig(NEW.lang)::regconfig, coalesce(NEW.title,   '')), 'A') ||
    setweight(to_tsvector(lang_to_tsconfig(NEW.lang)::regconfig, coalesce(NEW.summary, '')), 'B') ||
    setweight(to_tsvector(lang_to_tsconfig(NEW.lang)::regconfig, coalesce(NEW.body_md, '')), 'C');
  RETURN NEW;
END;
$$;

CREATE TRIGGER trg_post_translations_tsv
  BEFORE INSERT OR UPDATE ON post_translations
  FOR EACH ROW
  EXECUTE FUNCTION post_translations_tsv_update();

-- Backfill existing translation rows so the GIN index is populated from
-- the start. The trigger only fires on future writes; older rows need an
-- UPDATE to fire the BEFORE-UPDATE trigger and populate tsv.
UPDATE post_translations SET updated_at = updated_at;
