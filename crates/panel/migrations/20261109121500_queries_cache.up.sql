-- The answers store is the queries cache, open to everyone: its tables renamed, rows kept, and the
-- source kind `answers` gone with the keys it needed. `sources` is made again with the narrower
-- CHECKs, every other row copied as it is; nothing references it.

ALTER TABLE answers RENAME TO queries_cache;
ALTER TABLE answer_claims RENAME TO queries_cache_claims;

CREATE TABLE sources_new (
	key_id TEXT PRIMARY KEY CHECK (length(key_id) BETWEEN 1 AND 64 AND key_id GLOB '[a-z0-9]*' AND key_id NOT GLOB '*[^a-z0-9_-]*'),
	kind TEXT NOT NULL CHECK (kind IN ('site', 'review_archive', 'gbp', 'posthog', 'panel', 'sheet', 'telephony', 'bot')),
	-- A JSON array of brand ids, never empty.
	brand_ids TEXT NOT NULL CHECK (json_valid(brand_ids) AND json_type(brand_ids) = 'array' AND json_array_length(brand_ids) > 0),
	-- The HMAC secret, sealed under PANEL_DATA_KEY with the key id as associated data.
	secret_sealed BLOB NOT NULL,
	-- Which data key sealed it (a fingerprint, not the key), so a rotated key fails by name.
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	created_at INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000000 AS INTEGER)),
	revoked_at INTEGER,
	origin TEXT CHECK (origin IS NULL OR kind = 'site')
) STRICT;

INSERT INTO sources_new (key_id, kind, brand_ids, secret_sealed, data_key_fp, created_at, revoked_at, origin)
SELECT key_id, kind, brand_ids, secret_sealed, data_key_fp, created_at, revoked_at, origin
FROM sources
WHERE kind != 'answers';

DROP TABLE sources;
ALTER TABLE sources_new RENAME TO sources;

CREATE TRIGGER sources_no_delete BEFORE DELETE ON sources
BEGIN
	SELECT RAISE(ABORT, 'sources are never deleted: DELETE refused');
END;

CREATE TRIGGER sources_only_revoked BEFORE UPDATE ON sources
WHEN NEW.key_id IS NOT OLD.key_id OR NEW.kind IS NOT OLD.kind OR NEW.brand_ids IS NOT OLD.brand_ids
	OR NEW.secret_sealed IS NOT OLD.secret_sealed OR NEW.data_key_fp IS NOT OLD.data_key_fp OR NEW.created_at IS NOT OLD.created_at
	OR (NEW.origin IS NOT OLD.origin AND OLD.origin IS NOT NULL)
BEGIN
	SELECT RAISE(ABORT, 'a source only ever gets revoked, and its origin set once: UPDATE refused');
END;
