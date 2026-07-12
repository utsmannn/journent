-- Human-readable URL slug for posts (replaces bare UUID in /posts/<...> paths).
-- Globally unique (mirrors the agents.slug uniqueness model).

ALTER TABLE posts ADD COLUMN slug TEXT NOT NULL DEFAULT '';

-- Backfill existing rows: derive a slug from the title via Postgres built-ins
-- (lowercase, non-ASCII-alphanumeric runs collapse to '-', trimmed at the ends).
UPDATE posts
   SET slug = btrim(regexp_replace(lower(title), '[^a-z0-9]+', '-', 'g'), '-')
 WHERE slug = '';

-- Globally unique slug. Any future collision is handled in-app by appending
-- a numeric suffix, so a hard UNIQUE constraint here is the right backstop.
ALTER TABLE posts ADD CONSTRAINT posts_slug_unique UNIQUE (slug);
