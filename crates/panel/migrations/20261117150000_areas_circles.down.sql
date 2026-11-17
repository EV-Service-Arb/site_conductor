CREATE TABLE areas_old (
	slug TEXT PRIMARY KEY CHECK (slug GLOB '[a-z0-9]*' AND slug NOT GLOB '*[^a-z0-9-]*' AND length(slug) <= 64),
	kind TEXT NOT NULL CHECK (kind IN ('city', 'circle')),
	name TEXT NOT NULL CHECK (name = trim(name) AND name != ''),
	country TEXT NOT NULL CHECK (country GLOB '[A-Z][A-Z]'),
	place_id TEXT,
	lat REAL NOT NULL CHECK (lat BETWEEN -90 AND 90),
	lng REAL NOT NULL CHECK (lng BETWEEN -180 AND 180),
	radius_m INTEGER CHECK (radius_m > 0),
	south REAL,
	west REAL,
	north REAL,
	east REAL,
	margin_m INTEGER NOT NULL CHECK (margin_m >= 0),
	created_by BLOB NOT NULL REFERENCES users (user_id),
	created_at INTEGER NOT NULL,
	CHECK ((kind = 'city') = (place_id IS NOT NULL AND south IS NOT NULL AND west IS NOT NULL AND north IS NOT NULL AND east IS NOT NULL AND radius_m IS NULL)),
	CHECK ((kind = 'circle') = (place_id IS NULL AND south IS NULL AND west IS NULL AND north IS NULL AND east IS NULL AND radius_m IS NOT NULL)),
	CHECK (south IS NULL OR south <= north)
) STRICT;

INSERT INTO areas_old (slug, kind, name, country, lat, lng, radius_m, margin_m, created_by, created_at)
SELECT slug, 'circle', name, country, lat, lng, radius_m, 0, created_by, created_at FROM areas;

DROP TABLE areas;
ALTER TABLE areas_old RENAME TO areas;
