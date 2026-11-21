-- no-transaction
-- Each category back as a trade named its gcid, set up in its country as its head says; a GBP's
-- trade is its owner's trade of its gcid. The library, every version but the heads and every move
-- are dropped.

PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

CREATE TABLE trades (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	owner BLOB NOT NULL REFERENCES users (user_id),
	name TEXT NOT NULL CHECK (name = trim(name) AND name != ''),
	created_at INTEGER NOT NULL
) STRICT;

INSERT INTO trades (owner, name, created_at)
SELECT owner, trade, 0 FROM (SELECT owner, trade FROM categories UNION SELECT owner, trade FROM gmbs WHERE trade IS NOT NULL) ORDER BY owner, trade;

CREATE TABLE categories_old (
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

INSERT INTO categories_old (trade_id, country, keywords, google_primary, google_additional, excluded, demand)
SELECT t.id, c.country, json(v.body -> '$.keywords'), c.trade, json(v.body -> '$.additional'), json(v.body -> '$.excluded'), json(v.body -> '$.demand')
FROM categories c JOIN category_versions v ON v.id = c.head JOIN trades t ON t.owner = c.owner AND t.name = c.trade;

CREATE TABLE brands_old (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	owner BLOB NOT NULL REFERENCES users (user_id),
	gmail_id INTEGER NOT NULL REFERENCES gmails (id),
	trade_id INTEGER NOT NULL,
	country TEXT NOT NULL,
	name TEXT NOT NULL CHECK (name = trim(name) AND name != '' AND length(name) <= 100),
	site TEXT NOT NULL CHECK (site GLOB '[a-z0-9]*' AND site NOT GLOB '*[^a-z0-9_-]*' AND length(site) <= 64),
	-- PNG
	logo BLOB,
	logo_sha256 BLOB CHECK (length(logo_sha256) = 32),
	created_at INTEGER NOT NULL,
	removed_at INTEGER,
	CHECK ((logo IS NULL) = (logo_sha256 IS NULL)),
	FOREIGN KEY (trade_id, country) REFERENCES categories (trade_id, country)
) STRICT;

INSERT INTO brands_old (id, owner, gmail_id, trade_id, country, name, site, logo, logo_sha256, created_at, removed_at)
SELECT b.id, b.owner, b.gmail_id, t.id, b.country, b.name, b.site, b.logo, b.logo_sha256, b.created_at, b.removed_at
FROM brands b JOIN trades t ON t.owner = b.owner AND t.name = b.trade;

CREATE TABLE kit_drafts_old (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	trade_id INTEGER NOT NULL,
	country TEXT NOT NULL,
	text TEXT NOT NULL CHECK (text = trim(text) AND text != '' AND length(text) <= 4000),
	created_at INTEGER NOT NULL,
	removed_at INTEGER,
	FOREIGN KEY (trade_id, country) REFERENCES categories (trade_id, country)
) STRICT;

INSERT INTO kit_drafts_old (id, trade_id, country, text, created_at, removed_at)
SELECT d.id, t.id, d.country, d.text, d.created_at, d.removed_at FROM kit_drafts d JOIN trades t ON t.owner = d.owner AND t.name = d.trade;

CREATE TABLE gmbs_old (
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
	trade_id INTEGER REFERENCES trades (id), purged_at INTEGER CHECK (purged_at IS NULL OR removed_at IS NOT NULL), website TEXT, hours TEXT, google_primary TEXT, brand_id INTEGER REFERENCES brands (id), synced_at INTEGER, role TEXT CHECK (role IS NULL OR (role IN ('owner', 'manager') AND gmail_id IS NOT NULL)), owner_gmail TEXT CHECK (owner_gmail IS NULL OR (owner_gmail = lower(trim(owner_gmail)) AND owner_gmail != '' AND role = 'manager')),
	CHECK ((lat IS NULL) = (lng IS NULL) AND (placed_by IS NULL OR lat IS NOT NULL)),
	CHECK (country_by IS NULL OR country IS NOT NULL),
	CONSTRAINT a_draft_has_no_place CHECK (removed_at IS NOT NULL OR state != 'draft' OR place_id IS NULL),
	CONSTRAINT a_verified_gbp_has_a_place CHECK (removed_at IS NOT NULL OR state != 'verified' OR place_id IS NOT NULL),
	CONSTRAINT past_draft_a_gbp_has_a_gmail_and_a_country CHECK (removed_at IS NOT NULL OR state = 'draft' OR (gmail_id IS NOT NULL AND country IS NOT NULL))
) STRICT;

INSERT INTO gmbs_old (id, owner, gmail_id, place_id, name, address, state, country, country_by, phone, lat, lng, placed_by, created_at, removed_at,
	areas, hidden_address, customers_visit, film, trade_id, purged_at, website, hours, google_primary, brand_id, synced_at, role, owner_gmail)
SELECT g.id, g.owner, g.gmail_id, g.place_id, g.name, g.address, g.state, g.country, g.country_by, g.phone, g.lat, g.lng, g.placed_by, g.created_at, g.removed_at,
	g.areas, g.hidden_address, g.customers_visit, g.film, (SELECT t.id FROM trades t WHERE t.owner = g.owner AND t.name = g.trade), g.purged_at, g.website, g.hours,
	g.google_primary, g.brand_id, g.synced_at, g.role, g.owner_gmail
FROM gmbs g;

DROP VIEW category_lineage;
DROP TABLE library_seen;
DROP TABLE published_studies;
DROP TABLE categories;
DROP TABLE category_moves;
DROP TABLE listing_versions;
DROP TABLE listings;
DROP TABLE category_versions;
DROP TABLE gmbs;
ALTER TABLE gmbs_old RENAME TO gmbs;
DROP TABLE brands;
ALTER TABLE brands_old RENAME TO brands;
DROP TABLE kit_drafts;
ALTER TABLE kit_drafts_old RENAME TO kit_drafts;
ALTER TABLE categories_old RENAME TO categories;
DELETE FROM placement_pulls;

CREATE UNIQUE INDEX trades_name ON trades (owner, lower(name));

CREATE UNIQUE INDEX gmbs_name ON gmbs (name) WHERE removed_at IS NULL;

CREATE UNIQUE INDEX gmbs_place ON gmbs (place_id) WHERE removed_at IS NULL;

CREATE UNIQUE INDEX gmbs_phone ON gmbs (phone) WHERE removed_at IS NULL;

CREATE INDEX gmbs_owner ON gmbs (owner);

CREATE INDEX gmbs_trade ON gmbs (trade_id);

CREATE TRIGGER gmbs_no_delete BEFORE DELETE ON gmbs
BEGIN
	SELECT RAISE(ABORT, 'GMBs are never deleted, only removed: DELETE refused');
END;

CREATE TRIGGER gmbs_fixed BEFORE UPDATE ON gmbs
WHEN NEW.id IS NOT OLD.id OR NEW.owner IS NOT OLD.owner OR NEW.created_at IS NOT OLD.created_at
	OR (OLD.place_id IS NOT NULL AND NEW.place_id IS NOT OLD.place_id)
BEGIN
	SELECT RAISE(ABORT, 'a GMB''s id, owner, creation and place, once set, never change: UPDATE refused');
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

CREATE TRIGGER gmbs_purged_once BEFORE UPDATE OF purged_at ON gmbs
WHEN OLD.removed_at IS NULL AND NEW.purged_at IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'a live GMB is not purged: UPDATE refused');
END;

CREATE INDEX gmbs_brand ON gmbs (brand_id);

CREATE TRIGGER gmbs_brand_on_insert BEFORE INSERT ON gmbs
WHEN NEW.brand_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM brands b WHERE b.id = NEW.brand_id
	AND b.gmail_id IS NEW.gmail_id AND b.trade_id IS NEW.trade_id AND b.country IS NEW.country)
BEGIN
	SELECT RAISE(ABORT, 'a GBP''s brand is of its gmail and its category: INSERT refused');
END;

CREATE TRIGGER gmbs_brand_on_update BEFORE UPDATE OF brand_id, gmail_id, trade_id, country ON gmbs
WHEN NEW.brand_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM brands b WHERE b.id = NEW.brand_id
	AND b.gmail_id IS NEW.gmail_id AND b.trade_id IS NEW.trade_id AND b.country IS NEW.country)
BEGIN
	SELECT RAISE(ABORT, 'a GBP''s brand is of its gmail and its category: UPDATE refused');
END;

CREATE TRIGGER gmbs_removed_stays BEFORE UPDATE ON gmbs
WHEN OLD.removed_at IS NOT NULL AND NOT (
	NEW.id IS OLD.id AND NEW.owner IS OLD.owner AND NEW.gmail_id IS OLD.gmail_id AND NEW.place_id IS OLD.place_id
	AND NEW.name IS OLD.name AND NEW.address IS OLD.address AND NEW.state IS OLD.state AND NEW.country IS OLD.country
	AND NEW.country_by IS OLD.country_by AND NEW.phone IS OLD.phone AND NEW.lat IS OLD.lat AND NEW.lng IS OLD.lng
	AND NEW.placed_by IS OLD.placed_by AND NEW.created_at IS OLD.created_at
	AND NEW.areas IS OLD.areas AND NEW.hidden_address IS OLD.hidden_address AND NEW.customers_visit IS OLD.customers_visit
	AND NEW.film IS OLD.film AND NEW.role IS OLD.role AND NEW.owner_gmail IS OLD.owner_gmail AND (
		-- its trade or brand given up, so the trade can be deleted and the brand removed
		((NEW.trade_id IS NULL OR NEW.trade_id IS OLD.trade_id) AND (NEW.brand_id IS NULL OR NEW.brand_id IS OLD.brand_id)
			AND NEW.removed_at IS OLD.removed_at AND NEW.purged_at IS OLD.purged_at)
		OR (NEW.trade_id IS OLD.trade_id AND NEW.brand_id IS OLD.brand_id AND OLD.purged_at IS NULL AND (
			(NEW.removed_at IS NULL AND NEW.purged_at IS NULL)
			OR (NEW.removed_at IS OLD.removed_at AND NEW.purged_at IS NOT NULL)))))
BEGIN
	SELECT RAISE(ABORT, 'a removed GMB is restored until purged, purged once, or gives its trade or brand up: UPDATE refused');
END;

CREATE UNIQUE INDEX brands_name ON brands (gmail_id, trade_id, country, lower(name)) WHERE removed_at IS NULL;

CREATE INDEX brands_category ON brands (trade_id, country);

CREATE TRIGGER brands_owner BEFORE INSERT ON brands
WHEN NEW.owner IS NOT (SELECT owner FROM gmails WHERE id = NEW.gmail_id) OR NEW.owner IS NOT (SELECT owner FROM trades WHERE id = NEW.trade_id)
BEGIN
	SELECT RAISE(ABORT, 'a brand is of its owner''s gmail and trade: INSERT refused');
END;

CREATE TRIGGER brands_fixed BEFORE UPDATE ON brands
WHEN NEW.id IS NOT OLD.id OR NEW.owner IS NOT OLD.owner OR NEW.gmail_id IS NOT OLD.gmail_id OR NEW.trade_id IS NOT OLD.trade_id
	OR NEW.country IS NOT OLD.country OR NEW.created_at IS NOT OLD.created_at
BEGIN
	SELECT RAISE(ABORT, 'a brand''s gmail and category never change: UPDATE refused');
END;

CREATE TRIGGER brands_kept BEFORE DELETE ON brands BEGIN SELECT RAISE(ABORT, 'a brand is removed, not deleted'); END;

CREATE INDEX kit_drafts_category ON kit_drafts (trade_id, country);

CREATE TRIGGER kit_drafts_kept BEFORE DELETE ON kit_drafts BEGIN SELECT RAISE(ABORT, 'a draft is removed, not deleted'); END;

PRAGMA foreign_key_check;

COMMIT;

PRAGMA foreign_keys = ON;
