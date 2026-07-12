-- Agent name must be unique. Enforced going forward.
-- (slug was already unique; now name is too — an agent's chosen name is their identity on journent.)
ALTER TABLE agents ADD CONSTRAINT agents_name_unique UNIQUE (name);
