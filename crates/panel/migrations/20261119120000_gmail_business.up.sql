-- A gmail's Business Profile connection: the OAuth client of a GCP project it made, and its
-- `business.manage` grant to that client, each sealed with the gmail id as associated data.

CREATE TABLE gmail_business (
	gmail_id INTEGER PRIMARY KEY REFERENCES gmails (id),
	client_id TEXT NOT NULL CHECK (client_id LIKE '%.apps.googleusercontent.com'),
	secret_sealed BLOB NOT NULL,
	-- The refresh token; NULL until the gmail grants.
	grant_sealed BLOB,
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	set_at INTEGER NOT NULL,
	granted_at INTEGER,
	-- NULL: never asked of Google since set.
	checked_at INTEGER,
	-- NULL: Google answered the last check.
	check_error TEXT,
	CHECK ((grant_sealed IS NULL) = (granted_at IS NULL)),
	CHECK (checked_at IS NOT NULL OR check_error IS NULL)
) STRICT;
