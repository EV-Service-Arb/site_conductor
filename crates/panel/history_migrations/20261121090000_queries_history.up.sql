-- Every version of an answer the queries cache kept but the latest, which is the panel's
-- queries_cache row (docs/ARCHITECTURE.md, Queries cache). A version is never changed nor deleted.
CREATE TABLE queries_history (
	scheme TEXT NOT NULL CHECK (scheme IN ('places', 'gop')),
	key BLOB NOT NULL CHECK (length(key) = 32),
	fetched_at INTEGER NOT NULL,
	answer TEXT NOT NULL CHECK (json_valid(answer)),
	request TEXT NOT NULL,
	PRIMARY KEY (scheme, key, fetched_at)
) STRICT;

CREATE TRIGGER queries_history_no_update BEFORE UPDATE ON queries_history
BEGIN
	SELECT RAISE(ABORT, 'a version of an answer is never changed: UPDATE refused');
END;

CREATE TRIGGER queries_history_no_delete BEFORE DELETE ON queries_history
BEGIN
	SELECT RAISE(ABORT, 'a version of an answer is never dropped: DELETE refused');
END;
