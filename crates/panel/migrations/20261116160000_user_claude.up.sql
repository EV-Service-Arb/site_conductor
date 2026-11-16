-- A person's own Claude (docs/ARCHITECTURE.md, the assistant): its OAuth token, sealed under
-- PANEL_DATA_KEY with the user id as associated data. Their answers run on it.
CREATE TABLE user_claude (
	user_id BLOB PRIMARY KEY REFERENCES users (user_id),
	token_sealed BLOB NOT NULL,
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	set_at INTEGER NOT NULL,
	expires_at INTEGER NOT NULL,
	checked_at INTEGER NOT NULL,
	-- NULL: Claude took it last time.
	check_error TEXT
) STRICT;
