-- An area is a circle around its centre; `city` is the locality it was drawn from, a reference
-- outline only. A city's radius becomes half its viewport's height, and a margin joins the radius.
CREATE TABLE areas_new (
	slug TEXT PRIMARY KEY CHECK (slug GLOB '[a-z0-9]*' AND slug NOT GLOB '*[^a-z0-9-]*' AND length(slug) <= 64),
	name TEXT NOT NULL CHECK (name = trim(name) AND name != ''),
	country TEXT NOT NULL CHECK (country GLOB '[A-Z][A-Z]'),
	city TEXT,
	lat REAL NOT NULL CHECK (lat BETWEEN -90 AND 90),
	lng REAL NOT NULL CHECK (lng BETWEEN -180 AND 180),
	radius_m INTEGER NOT NULL CHECK (radius_m > 0),
	created_by BLOB NOT NULL REFERENCES users (user_id),
	created_at INTEGER NOT NULL
) STRICT;

INSERT INTO areas_new (slug, name, country, city, lat, lng, radius_m, created_by, created_at)
SELECT slug, name, country, place_id, lat, lng,
	CASE kind WHEN 'city' THEN CAST(round((north - south) * 111320 / 2) AS INTEGER) ELSE radius_m END + margin_m,
	created_by, created_at
FROM areas;

DROP TABLE areas;
ALTER TABLE areas_new RENAME TO areas;
