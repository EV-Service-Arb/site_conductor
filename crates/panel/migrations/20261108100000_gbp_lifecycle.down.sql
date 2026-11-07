-- no-transaction
-- Back to a GMB under a gmail on a place: drafts are dropped, the points go back to
-- `place_points` by place, phones and states are lost.

PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

DELETE FROM gmbs_off WHERE gmb_id IN (SELECT id FROM gmbs WHERE place_id IS NULL);

CREATE TABLE gmbs_old (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	gmail_id INTEGER NOT NULL REFERENCES gmails (id),
	place_id TEXT NOT NULL CHECK (length(place_id) > 0),
	name TEXT,
	address TEXT,
	gbp_account TEXT,
	gbp_location TEXT,
	created_at INTEGER NOT NULL,
	removed_at INTEGER,
	country TEXT CHECK (country GLOB '[A-Z][A-Z]'),
	country_by BLOB REFERENCES users (user_id)
) STRICT;

INSERT INTO gmbs_old (id, gmail_id, place_id, name, address, created_at, removed_at, country, country_by)
SELECT id, gmail_id, place_id, name, address, created_at, removed_at, country, country_by FROM gmbs WHERE place_id IS NOT NULL;

CREATE TABLE place_points (
	place_id TEXT PRIMARY KEY,
	lat REAL NOT NULL,
	lng REAL NOT NULL,
	placed_by BLOB REFERENCES users (user_id)
) STRICT;

INSERT INTO place_points (place_id, lat, lng, placed_by)
SELECT place_id, lat, lng, placed_by FROM gmbs WHERE place_id IS NOT NULL AND lat IS NOT NULL ORDER BY removed_at IS NULL DESC, id
ON CONFLICT (place_id) DO NOTHING;

UPDATE sqlite_sequence SET seq = (SELECT seq FROM sqlite_sequence WHERE name = 'gmbs') WHERE name = 'gmbs_old';

DROP TABLE gmbs;
ALTER TABLE gmbs_old RENAME TO gmbs;

CREATE UNIQUE INDEX gmbs_gmail_place ON gmbs (gmail_id, place_id) WHERE removed_at IS NULL;

CREATE TRIGGER gmbs_no_delete BEFORE DELETE ON gmbs
BEGIN
	SELECT RAISE(ABORT, 'GMBs are never deleted, only removed: DELETE refused');
END;

CREATE TRIGGER gmbs_only_removed BEFORE UPDATE ON gmbs
WHEN NEW.id IS NOT OLD.id OR NEW.gmail_id IS NOT OLD.gmail_id OR NEW.place_id IS NOT OLD.place_id OR NEW.name IS NOT OLD.name
	OR NEW.address IS NOT OLD.address OR NEW.gbp_account IS NOT OLD.gbp_account OR NEW.gbp_location IS NOT OLD.gbp_location
	OR NEW.created_at IS NOT OLD.created_at OR OLD.removed_at IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'a GMB is only removed, once, and its country set: UPDATE refused');
END;

COMMIT;

PRAGMA foreign_keys = ON;
