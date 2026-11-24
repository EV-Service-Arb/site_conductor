-- A person's own address is a destination from the start, once: one they removed is not added again.
ALTER TABLE alert_access ADD COLUMN mail_started INTEGER NOT NULL DEFAULT 0 CHECK (mail_started IN (0, 1));

-- The tests sent for a person, for a minute: what bounds how many.
CREATE TABLE alert_tests (
	user_id BLOB NOT NULL CHECK (length(user_id) = 16),
	at INTEGER NOT NULL
) STRICT;
