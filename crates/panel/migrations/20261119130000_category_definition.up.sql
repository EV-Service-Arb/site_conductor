-- no-transaction
-- A category is the whole of what its studies read: its keywords weighed against the primary's
-- name, a weight on each additional Google category (its competitor tier), the excluded words and
-- types, and its demand. Placement's trade files, and the `study` naming one, are gone.
-- The primary's name becomes a keyword at weight 1, by `names` (Google's name in the country's
-- language, lowercased as sa_core::gbp::keywords stores it); a primary `names` lacks stops the
-- migration, as does an additional category: its weight is a person's to give.
-- kits and brands reference categories: the copy runs with foreign keys off outside a transaction, as 20261007090000_bot_source_kind; rows keep their keys.

PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

CREATE TEMP TABLE names (country TEXT NOT NULL, gcid TEXT NOT NULL, keyword TEXT NOT NULL, PRIMARY KEY (country, gcid));
INSERT INTO names VALUES
	('FR', 'gcid:plumber', 'plombier'),
	('FR', 'gcid:house_cleaning_service', 'service de ménage'),
	('FR', 'gcid:car_detailing_service', 'service d''esthétique automobile'),
	('US', 'gcid:plumber', 'plumber');

CREATE TABLE categories_new (
	trade_id INTEGER NOT NULL REFERENCES trades (id) ON DELETE CASCADE,
	country TEXT NOT NULL CHECK (country GLOB '[A-Z][A-Z]'),
	-- [{text, weight}], text in sa_core::gbp::keywords' form, weight null or positive.
	keywords TEXT NOT NULL CHECK (json_valid(keywords) AND json_type(keywords) = 'array' AND json_array_length(keywords) <= 50),
	google_primary TEXT CHECK (google_primary GLOB 'gcid:*'),
	-- [{id, weight}] in the order they are entered into Google's form.
	google_additional TEXT NOT NULL CHECK (json_valid(google_additional) AND json_type(google_additional) = 'array'
		AND json_array_length(google_additional) <= 9 AND (google_primary IS NOT NULL OR json_array_length(google_additional) = 0)),
	-- sa_core::placement::Excluded.
	excluded TEXT NOT NULL CHECK (json_valid(excluded) AND json_type(excluded) = 'object'),
	-- sa_core::placement::Demand, or none.
	demand TEXT CHECK (demand IS NULL OR (json_valid(demand) AND json_type(demand) = 'object')),
	PRIMARY KEY (trade_id, country)
) STRICT;

CREATE TEMP TRIGGER categories_unnamed BEFORE INSERT ON categories_new
WHEN NEW.google_primary IS NOT NULL AND NOT EXISTS (SELECT 1 FROM names n WHERE n.country = NEW.country AND n.gcid = NEW.google_primary)
BEGIN
	SELECT RAISE(ABORT, 'primary Google categories without their name, to add to this migration''s names: ' || (
		SELECT group_concat(c.country || ' ' || c.google_primary, ', ') FROM categories c
		WHERE c.google_primary IS NOT NULL AND NOT EXISTS (SELECT 1 FROM names n WHERE n.country = c.country AND n.gcid = c.google_primary)));
END;

CREATE TEMP TRIGGER categories_unweighed BEFORE INSERT ON categories_new
WHEN EXISTS (SELECT 1 FROM categories c WHERE json_array_length(c.google_additional) > 0)
BEGIN
	SELECT RAISE(ABORT, 'additional Google categories need a weight each: take them off these categories, migrate, and give them back with one: ' || (
		SELECT group_concat(c.trade_id || '·' || c.country, ', ') FROM categories c WHERE json_array_length(c.google_additional) > 0));
END;

INSERT INTO categories_new (trade_id, country, keywords, google_primary, google_additional, excluded, demand)
SELECT c.trade_id, c.country,
	(SELECT json_group_array(json(k.value)) FROM (
		SELECT json_object('text', j.value, 'weight', CASE WHEN j.value = n.keyword THEN 1.0 ELSE NULL END) AS value
		FROM json_each(c.keywords) j
		UNION ALL
		SELECT json_object('text', n.keyword, 'weight', 1.0)
		WHERE n.keyword IS NOT NULL AND NOT EXISTS (SELECT 1 FROM json_each(c.keywords) j WHERE j.value = n.keyword)
	) k),
	c.google_primary, '[]', '{"words":[],"types":[]}', NULL
FROM categories c LEFT JOIN names n ON n.country = c.country AND n.gcid = c.google_primary;

DROP TRIGGER categories_unnamed;
DROP TRIGGER categories_unweighed;
DROP TABLE names;
DROP TABLE categories;
ALTER TABLE categories_new RENAME TO categories;

COMMIT;

PRAGMA foreign_keys = ON;
