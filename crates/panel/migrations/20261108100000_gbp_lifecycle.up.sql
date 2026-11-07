-- no-transaction
-- A GBP moves through draft → verifying → verified ⇄ suspended (docs/ARCHITECTURE.md, Tokens and
-- GMBs). A draft has no place on Google, and may have no gmail yet: the owner moves onto the
-- row, and its point with it (`place_points` keyed a point by place). The gmail's owner stays the
-- GBP's. Unique among live rows: the name, the place.
--
-- SQLite cannot alter a NOT NULL or a CHECK: the table is made again, with foreign keys off
-- outside a transaction (`gmbs_off` references it), as 20261007090000_bot_source_kind did. Ids
-- are kept. Every live row becomes verified; one without a name, a country, or doubling another's
-- name or place fails the migration. A removed gmail's GMBs, which counted as removed, are.

PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

CREATE TABLE gmbs_new (
	-- The id every service stores; never reused.
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	owner BLOB NOT NULL REFERENCES users (user_id),
	gmail_id INTEGER REFERENCES gmails (id),
	place_id TEXT CHECK (length(place_id) > 0),
	name TEXT NOT NULL CHECK (name = trim(name) AND name != ''),
	address TEXT,
	state TEXT NOT NULL CHECK (state IN ('draft', 'verifying', 'verified', 'suspended')),
	-- Google's (country_by NULL), else set by hand while Google has none.
	country TEXT CHECK (country GLOB '[A-Z][A-Z]'),
	country_by BLOB REFERENCES users (user_id),
	phone TEXT CHECK (phone = trim(phone) AND phone != ''),
	-- Google's (placed_by NULL), else placed by hand on the map while Google has none.
	lat REAL,
	lng REAL,
	placed_by BLOB REFERENCES users (user_id),
	created_at INTEGER NOT NULL,
	removed_at INTEGER,
	CHECK ((lat IS NULL) = (lng IS NULL) AND (placed_by IS NULL OR lat IS NOT NULL)),
	CHECK (country_by IS NULL OR country IS NOT NULL),
	CONSTRAINT a_draft_and_only_a_draft_has_no_place CHECK (removed_at IS NOT NULL OR (state = 'draft') = (place_id IS NULL)),
	CONSTRAINT a_placed_gbp_has_a_gmail_and_a_country CHECK (removed_at IS NOT NULL OR state = 'draft' OR (gmail_id IS NOT NULL AND country IS NOT NULL))
) STRICT;

INSERT INTO gmbs_new (id, owner, gmail_id, place_id, name, address, state, country, country_by, lat, lng, placed_by, created_at, removed_at)
SELECT b.id, g.owner, b.gmail_id, b.place_id, b.name, b.address, 'verified', b.country, b.country_by, p.lat, p.lng, p.placed_by, b.created_at,
	coalesce(b.removed_at, g.removed_at)
FROM gmbs b JOIN gmails g ON g.id = b.gmail_id LEFT JOIN place_points p ON p.place_id = b.place_id;

UPDATE sqlite_sequence SET seq = (SELECT seq FROM sqlite_sequence WHERE name = 'gmbs') WHERE name = 'gmbs_new';

DROP TABLE gmbs;
DROP TABLE place_points;
ALTER TABLE gmbs_new RENAME TO gmbs;

CREATE UNIQUE INDEX gmbs_name ON gmbs (name) WHERE removed_at IS NULL;
CREATE UNIQUE INDEX gmbs_place ON gmbs (place_id) WHERE removed_at IS NULL;
CREATE INDEX gmbs_owner ON gmbs (owner);

CREATE TRIGGER gmbs_no_delete BEFORE DELETE ON gmbs
BEGIN
	SELECT RAISE(ABORT, 'GMBs are never deleted, only removed: DELETE refused');
END;

CREATE TRIGGER gmbs_removed_stays BEFORE UPDATE ON gmbs
WHEN OLD.removed_at IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'a removed GMB changes no more: UPDATE refused');
END;

CREATE TRIGGER gmbs_fixed BEFORE UPDATE ON gmbs
WHEN NEW.id IS NOT OLD.id OR NEW.owner IS NOT OLD.owner OR NEW.created_at IS NOT OLD.created_at
	OR (OLD.place_id IS NOT NULL AND NEW.place_id IS NOT OLD.place_id)
BEGIN
	SELECT RAISE(ABORT, 'a GMB''s id, owner, creation and place, once set, never change: UPDATE refused');
END;

CREATE TRIGGER gmbs_named_as_draft BEFORE UPDATE OF name ON gmbs
WHEN NEW.name IS NOT OLD.name AND OLD.state != 'draft'
BEGIN
	SELECT RAISE(ABORT, 'a GMB is renamed only while a draft: UPDATE refused');
END;

CREATE TRIGGER gmbs_state BEFORE UPDATE OF state ON gmbs
WHEN NEW.state IS NOT OLD.state AND OLD.state || '>' || NEW.state NOT IN
	('draft>verifying', 'verifying>verified', 'verified>suspended', 'verified>verifying', 'suspended>verified', 'suspended>verifying')
BEGIN
	SELECT RAISE(ABORT, 'a GMB''s state moves only as sa_core::gbp::State::next allows: UPDATE refused');
END;

CREATE TRIGGER gmbs_gmail_owner_on_insert BEFORE INSERT ON gmbs
WHEN NEW.gmail_id IS NOT NULL AND NEW.owner IS NOT (SELECT owner FROM gmails WHERE id = NEW.gmail_id)
BEGIN
	SELECT RAISE(ABORT, 'a GMB''s gmail is its owner''s: INSERT refused');
END;

CREATE TRIGGER gmbs_gmail_owner_on_update BEFORE UPDATE OF gmail_id ON gmbs
WHEN NEW.gmail_id IS NOT NULL AND NEW.owner IS NOT (SELECT owner FROM gmails WHERE id = NEW.gmail_id)
BEGIN
	SELECT RAISE(ABORT, 'a GMB''s gmail is its owner''s: UPDATE refused');
END;

COMMIT;

PRAGMA foreign_keys = ON;
