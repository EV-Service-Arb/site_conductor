-- no-transaction
-- A GBP's place is certain only once it is Verified (docs/ARCHITECTURE.md, Tokens and GBPs): one
-- verifying or suspended is not on Maps, so it may have none. A draft still has none; past a draft
-- a GBP has its gmail and its country, as before.
--
-- SQLite cannot alter a CHECK: the table is made again, as 20261108100000_gbp_lifecycle did, its
-- indexes and triggers with it. Ids and every column are kept.

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
	areas TEXT CHECK (json_valid(areas) AND json_type(areas) = 'array' AND json_array_length(areas) BETWEEN 1 AND 20),
	-- Where the verification film is shot; shown to customers only by a storefront (customers_visit).
	hidden_address TEXT CHECK (hidden_address = trim(hidden_address) AND hidden_address != ''),
	customers_visit INTEGER CHECK (customers_visit IN (0, 1)),
	film TEXT CHECK (film = trim(film) AND film != ''),
	trade_id INTEGER REFERENCES trades (id),
	CHECK ((lat IS NULL) = (lng IS NULL) AND (placed_by IS NULL OR lat IS NOT NULL)),
	CHECK (country_by IS NULL OR country IS NOT NULL),
	CONSTRAINT a_draft_has_no_place CHECK (removed_at IS NOT NULL OR state != 'draft' OR place_id IS NULL),
	CONSTRAINT a_verified_gbp_has_a_place CHECK (removed_at IS NOT NULL OR state != 'verified' OR place_id IS NOT NULL),
	CONSTRAINT past_draft_a_gbp_has_a_gmail_and_a_country CHECK (removed_at IS NOT NULL OR state = 'draft' OR (gmail_id IS NOT NULL AND country IS NOT NULL))
) STRICT;

INSERT INTO gmbs_new (id, owner, gmail_id, place_id, name, address, state, country, country_by, phone, lat, lng, placed_by, created_at, removed_at,
	areas, hidden_address, customers_visit, film, trade_id)
SELECT id, owner, gmail_id, place_id, name, address, state, country, country_by, phone, lat, lng, placed_by, created_at, removed_at,
	areas, hidden_address, customers_visit, film, trade_id
FROM gmbs;

UPDATE sqlite_sequence SET seq = (SELECT seq FROM sqlite_sequence WHERE name = 'gmbs') WHERE name = 'gmbs_new';

DROP TABLE gmbs;
ALTER TABLE gmbs_new RENAME TO gmbs;

CREATE UNIQUE INDEX gmbs_name ON gmbs (name) WHERE removed_at IS NULL;
CREATE UNIQUE INDEX gmbs_place ON gmbs (place_id) WHERE removed_at IS NULL;
CREATE UNIQUE INDEX gmbs_phone ON gmbs (phone) WHERE removed_at IS NULL;
CREATE INDEX gmbs_owner ON gmbs (owner);
CREATE INDEX gmbs_trade ON gmbs (trade_id);

CREATE TRIGGER gmbs_no_delete BEFORE DELETE ON gmbs
BEGIN
	SELECT RAISE(ABORT, 'GMBs are never deleted, only removed: DELETE refused');
END;

CREATE TRIGGER gmbs_removed_stays BEFORE UPDATE ON gmbs
WHEN OLD.removed_at IS NOT NULL AND NOT (NEW.trade_id IS NULL
	AND NEW.id IS OLD.id AND NEW.owner IS OLD.owner AND NEW.gmail_id IS OLD.gmail_id AND NEW.place_id IS OLD.place_id
	AND NEW.name IS OLD.name AND NEW.address IS OLD.address AND NEW.state IS OLD.state AND NEW.country IS OLD.country
	AND NEW.country_by IS OLD.country_by AND NEW.phone IS OLD.phone AND NEW.lat IS OLD.lat AND NEW.lng IS OLD.lng
	AND NEW.placed_by IS OLD.placed_by AND NEW.created_at IS OLD.created_at AND NEW.removed_at IS OLD.removed_at
	AND NEW.areas IS OLD.areas AND NEW.hidden_address IS OLD.hidden_address AND NEW.customers_visit IS OLD.customers_visit
	AND NEW.film IS OLD.film)
BEGIN
	SELECT RAISE(ABORT, 'a removed GMB changes no more but to give its trade up: UPDATE refused');
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

CREATE TRIGGER gmbs_trade_owner_on_insert BEFORE INSERT ON gmbs
WHEN NEW.trade_id IS NOT NULL AND NEW.owner IS NOT (SELECT owner FROM trades WHERE id = NEW.trade_id)
BEGIN
	SELECT RAISE(ABORT, 'a GMB''s trade is its owner''s: INSERT refused');
END;

CREATE TRIGGER gmbs_trade_owner_on_update BEFORE UPDATE OF trade_id ON gmbs
WHEN NEW.trade_id IS NOT NULL AND NEW.owner IS NOT (SELECT owner FROM trades WHERE id = NEW.trade_id)
BEGIN
	SELECT RAISE(ABORT, 'a GMB''s trade is its owner''s: UPDATE refused');
END;

COMMIT;

PRAGMA foreign_keys = ON;
