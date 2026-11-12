-- The additional categories are lost; the primary's gcid stands as the label.

CREATE TABLE categories_old (
	trade_id INTEGER NOT NULL REFERENCES trades (id) ON DELETE CASCADE,
	country TEXT NOT NULL CHECK (country GLOB '[A-Z][A-Z]'),
	-- In sa_core::gbp::keywords' form.
	keywords TEXT NOT NULL CHECK (json_valid(keywords) AND json_type(keywords) = 'array' AND json_array_length(keywords) <= 50),
	-- The label typed into Google's form, in the country's language.
	google_category TEXT CHECK (google_category = trim(google_category) AND google_category != ''),
	-- Placement's trade that studies it.
	study TEXT CHECK (study = trim(study) AND study != ''),
	PRIMARY KEY (trade_id, country)
) STRICT;
INSERT INTO categories_old (trade_id, country, keywords, google_category, study)
SELECT trade_id, country, keywords, google_primary, study FROM categories;
DROP TABLE categories;
ALTER TABLE categories_old RENAME TO categories;
