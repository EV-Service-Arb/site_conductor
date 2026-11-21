CREATE TABLE queries_cache_new (
	scheme TEXT NOT NULL CHECK (scheme IN ('places', 'gop')),
	key BLOB NOT NULL CHECK (length(key) = 32),
	answer TEXT NOT NULL CHECK (json_valid(answer)),
	request TEXT,
	fetched_at INTEGER NOT NULL,
	PRIMARY KEY (scheme, key)
) STRICT;

INSERT INTO queries_cache_new (scheme, key, answer, request, fetched_at)
SELECT scheme, key, answer, request, fetched_at
FROM queries_cache;

DROP TABLE queries_cache;
ALTER TABLE queries_cache_new RENAME TO queries_cache;
