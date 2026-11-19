-- no-transaction
-- Keywords back to their texts, additional categories to their ids; the weights, the excluded and
-- the demand are lost, and no category names a study.
-- kits and brands reference categories: the copy runs with foreign keys off outside a transaction, as 20261007090000_bot_source_kind; rows keep their keys.

PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

CREATE TABLE categories_old (
	trade_id INTEGER NOT NULL REFERENCES trades (id) ON DELETE CASCADE,
	country TEXT NOT NULL CHECK (country GLOB '[A-Z][A-Z]'),
	keywords TEXT NOT NULL CHECK (json_valid(keywords) AND json_type(keywords) = 'array' AND json_array_length(keywords) <= 50),
	google_primary TEXT CHECK (google_primary GLOB 'gcid:*'),
	google_additional TEXT NOT NULL CHECK (json_valid(google_additional) AND json_type(google_additional) = 'array'
		AND json_array_length(google_additional) <= 9 AND (google_primary IS NOT NULL OR json_array_length(google_additional) = 0)),
	study TEXT CHECK (study = trim(study) AND study != ''),
	PRIMARY KEY (trade_id, country)
) STRICT;

INSERT INTO categories_old (trade_id, country, keywords, google_primary, google_additional, study)
SELECT trade_id, country,
	(SELECT json_group_array(json_extract(j.value, '$.text')) FROM json_each(keywords) j),
	google_primary,
	(SELECT json_group_array(json_extract(j.value, '$.id')) FROM json_each(google_additional) j),
	NULL
FROM categories;

DROP TABLE categories;
ALTER TABLE categories_old RENAME TO categories;

COMMIT;

PRAGMA foreign_keys = ON;
