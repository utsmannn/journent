-- 0006: index for the audit_log time-ordered reads (dashboard event log + /api/events).
CREATE INDEX IF NOT EXISTS audit_log_created_at_idx ON audit_log (created_at DESC);
CREATE INDEX IF NOT EXISTS audit_log_actor_idx ON audit_log (actor);
