-- MCP clients and their grants (docs/ARCHITECTURE.md, Agents over MCP): a client registers
-- itself (RFC 7591), a user's consent opens a session for it, and a code hands the client
-- that session's id, its bearer, once.
CREATE TABLE oauth_clients (
	client_id TEXT PRIMARY KEY CHECK (length(client_id) = 64),
	-- A JSON array of URIs, never empty.
	redirect_uris TEXT NOT NULL CHECK (json_valid(redirect_uris) AND json_type(redirect_uris) = 'array' AND json_array_length(redirect_uris) > 0),
	name TEXT CHECK (length(name) <= 200),
	created_at INTEGER NOT NULL
) STRICT;

-- The client a session was granted to; NULL: a browser's. No foreign key, so the down
-- migration can drop it: a client is never deleted.
ALTER TABLE sessions ADD COLUMN client_id TEXT;

-- A code redeems once, within its minute; one never redeemed takes its session with it.
CREATE TABLE oauth_codes (
	code_hash BLOB PRIMARY KEY CHECK (length(code_hash) = 32),
	session BLOB NOT NULL REFERENCES sessions (id_hash) ON DELETE CASCADE,
	redirect_uri TEXT NOT NULL,
	challenge TEXT NOT NULL,
	-- The session's id, sealed under PANEL_DATA_KEY, the code's hash bound as associated data.
	bearer_sealed BLOB NOT NULL,
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	expires_at INTEGER NOT NULL
) STRICT;

CREATE INDEX oauth_codes_by_expiry ON oauth_codes (expires_at);
