-- What a Sync copies of a GBP's profile on Google: Places (New)'s fields, and its photos.
ALTER TABLE gmbs ADD COLUMN website TEXT;
-- JSON: a line a weekday, Monday first.
ALTER TABLE gmbs ADD COLUMN hours TEXT;
-- `gcid:plumber`: Google's primary category, when the catalog knows it.
ALTER TABLE gmbs ADD COLUMN google_primary TEXT;

-- A photo's id is never reused: its address is served immutable.
CREATE TABLE gmb_photos (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	n INTEGER NOT NULL,
	bytes BLOB NOT NULL,
	author TEXT,
	UNIQUE (gmb_id, n)
) STRICT;

-- outside a draft the name is Google's, written by a Sync; edit_gmb refuses a hand edit
DROP TRIGGER gmbs_named_as_draft;
