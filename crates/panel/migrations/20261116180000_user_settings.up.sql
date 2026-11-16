-- No row: asked above 100 tokens.
CREATE TABLE user_settings (
	user_id BLOB PRIMARY KEY REFERENCES users (user_id),
	confirm_above INTEGER NOT NULL CHECK (confirm_above >= 0)
) STRICT;
