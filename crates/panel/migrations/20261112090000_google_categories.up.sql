-- A trade's Google categories are gcids off Google's list (crates/panel/google_categories.json): the
-- primary, and the additional ones in the order they are entered into Google's form. A free-text
-- label maps by its exact name in its country's language (a gcid, as the down migration leaves it,
-- stays); one `labels` lacks stops the migration.

CREATE TEMP TABLE labels (country TEXT NOT NULL, label TEXT NOT NULL, gcid TEXT NOT NULL, PRIMARY KEY (country, label));
INSERT INTO labels VALUES ('FR', 'Plombier', 'gcid:plumber');

CREATE TABLE categories_new (
	trade_id INTEGER NOT NULL REFERENCES trades (id) ON DELETE CASCADE,
	country TEXT NOT NULL CHECK (country GLOB '[A-Z][A-Z]'),
	-- In sa_core::gbp::keywords' form.
	keywords TEXT NOT NULL CHECK (json_valid(keywords) AND json_type(keywords) = 'array' AND json_array_length(keywords) <= 50),
	google_primary TEXT CHECK (google_primary GLOB 'gcid:*'),
	google_additional TEXT NOT NULL CHECK (json_valid(google_additional) AND json_type(google_additional) = 'array'
		AND json_array_length(google_additional) <= 9 AND (google_primary IS NOT NULL OR json_array_length(google_additional) = 0)),
	-- Placement's trade that studies it.
	study TEXT CHECK (study = trim(study) AND study != ''),
	PRIMARY KEY (trade_id, country)
) STRICT;

CREATE TEMP TRIGGER categories_unmapped BEFORE INSERT ON categories_new
WHEN NEW.google_primary IS NOT NULL AND NEW.google_primary NOT IN (SELECT gcid FROM labels)
BEGIN
	SELECT RAISE(ABORT, 'Google categories without a gcid, to add to this migration''s labels: ' || (
		SELECT group_concat(c.country || ' ''' || c.google_category || '''', ', ') FROM categories c
		WHERE c.google_category IS NOT NULL AND NOT EXISTS (SELECT 1 FROM labels l WHERE l.country = c.country AND c.google_category IN (l.label, l.gcid))));
END;

INSERT INTO categories_new (trade_id, country, keywords, google_primary, google_additional, study)
SELECT c.trade_id, c.country, c.keywords, coalesce(l.gcid, c.google_category), '[]', c.study
FROM categories c LEFT JOIN labels l ON l.country = c.country AND l.label = c.google_category;

DROP TRIGGER categories_unmapped;
DROP TABLE labels;
DROP TABLE categories;
ALTER TABLE categories_new RENAME TO categories;
