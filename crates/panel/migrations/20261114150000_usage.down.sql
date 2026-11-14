CREATE TABLE street_views (
	id INTEGER PRIMARY KEY,
	user_id BLOB NOT NULL REFERENCES users (user_id),
	at INTEGER NOT NULL,
	lat REAL NOT NULL,
	lng REAL NOT NULL
) STRICT;
CREATE INDEX street_views_at ON street_views (at);
INSERT INTO street_views (user_id, at, lat, lng)
SELECT user_id, at, CAST(substr(key, 1, instr(key, ',') - 1) AS REAL), CAST(substr(key, instr(key, ',') + 1) AS REAL)
FROM usage WHERE item = 'street_view.panorama' AND user_id IS NOT NULL;
DROP TABLE unit_costs;
DROP TABLE usage_cursors;
DROP TABLE usage;
