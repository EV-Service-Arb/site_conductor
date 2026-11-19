-- What a grant's client does without asking its user first (sa_core::agent::Approval); NULL on a browser's.
ALTER TABLE sessions ADD COLUMN approval TEXT CHECK (approval IN ('ask', 'spend', 'act'));
UPDATE sessions SET approval = 'ask' WHERE client_id IS NOT NULL;
