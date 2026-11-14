-- Each Street View panorama opened on the browser's Maps key: none opens past Google's free ones
-- in a month (docs/ARCHITECTURE.md, Street View).
CREATE TABLE street_views (
	id INTEGER PRIMARY KEY,
	user_id BLOB NOT NULL REFERENCES users (user_id),
	at INTEGER NOT NULL,
	lat REAL NOT NULL,
	lng REAL NOT NULL
) STRICT;

CREATE INDEX street_views_at ON street_views (at);
