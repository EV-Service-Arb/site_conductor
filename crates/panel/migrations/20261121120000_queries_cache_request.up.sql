-- Every answer keeps the request it answers (site_conductor#126): a state still holding one without
-- refuses this migration (NOT NULL), and a fresh copy of prod's (`--reset --from-prod`) is its fix.
CREATE TABLE queries_cache_new (
	scheme TEXT NOT NULL CHECK (scheme IN ('places', 'gop')),
	key BLOB NOT NULL CHECK (length(key) = 32),
	answer TEXT NOT NULL CHECK (json_valid(answer)),
	request TEXT NOT NULL,
	fetched_at INTEGER NOT NULL,
	PRIMARY KEY (scheme, key)
) STRICT;

INSERT INTO queries_cache_new (scheme, key, answer, request, fetched_at)
SELECT scheme, key, answer, request, fetched_at
FROM queries_cache;

DROP TABLE queries_cache;
ALTER TABLE queries_cache_new RENAME TO queries_cache;
