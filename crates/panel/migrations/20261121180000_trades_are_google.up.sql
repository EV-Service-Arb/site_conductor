-- no-transaction
-- A trade is one of Google's categories (a gcid); a category is a person's settings for it in a
-- country, kept as immutable versions their moves point a head at; a library releases versions as
-- listings, and studies on released versions (docs/ARCHITECTURE.md, Library).
-- Each old category becomes its owner's category of its primary Google category, one version, one
-- move. A GBP's gcid is its trade's category's in its country, else the trade's only gcid. A
-- category without a primary, two trades of one owner on one gcid in one country, and a GBP whose
-- trade maps to no gcid or to several stop the migration, named.
-- Study ids hashed the retired trade ids, so placement_pulls' resume state is dropped.
-- gmbs, brands and kit_drafts are rebuilt with foreign keys off outside a transaction, as
-- 20261119130000_category_definition; rows keep their ids.

PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

CREATE TEMP TABLE refused (why TEXT NOT NULL);
CREATE TEMP TRIGGER refused_stops BEFORE INSERT ON refused BEGIN SELECT RAISE(ABORT, NEW.why); END;

INSERT INTO refused SELECT 'categories without a primary Google category, to give one or remove: ' || group_concat(t.name || '·' || c.country, ', ')
FROM categories c JOIN trades t ON t.id = c.trade_id WHERE c.google_primary IS NULL HAVING count(*) > 0;

INSERT INTO refused SELECT 'trades of one owner on one Google category in one country, to merge: ' || group_concat(names, '; ')
FROM (SELECT c.google_primary || '·' || c.country || ' (' || group_concat(t.name, ', ') || ')' AS names
	FROM categories c JOIN trades t ON t.id = c.trade_id GROUP BY t.owner, c.google_primary, c.country HAVING count(*) > 1)
HAVING count(*) > 0;

CREATE TEMP TABLE gmb_trade AS
SELECT g.id AS gmb, g.name, t.name AS trade_name, COALESCE(
	(SELECT c.google_primary FROM categories c WHERE c.trade_id = g.trade_id AND c.country = g.country),
	(SELECT max(c.google_primary) FROM categories c WHERE c.trade_id = g.trade_id HAVING count(DISTINCT c.google_primary) = 1)
) AS gcid
FROM gmbs g JOIN trades t ON t.id = g.trade_id;

INSERT INTO refused SELECT 'GBPs whose trade maps to no Google category or to several, to set up in their country: '
	|| group_concat(gmb || ' ' || quote(name) || ' (' || trade_name || ')', ', ')
FROM gmb_trade WHERE gcid IS NULL HAVING count(*) > 0;

CREATE TABLE category_versions (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	trade TEXT NOT NULL CHECK (trade GLOB 'gcid:?*'),
	country TEXT NOT NULL CHECK (country GLOB '[A-Z][A-Z]'),
	-- sa_panel::categories::Body: {keywords, additional, excluded, demand}, by gcid.
	body TEXT NOT NULL CHECK (json_valid(body) AND json_type(body) = 'object'),
	author BLOB NOT NULL REFERENCES users (user_id),
	at INTEGER NOT NULL,
	UNIQUE (id, trade, country)
) STRICT;

CREATE TRIGGER category_versions_no_update BEFORE UPDATE ON category_versions
BEGIN
	SELECT RAISE(ABORT, 'a category version is immutable: UPDATE refused');
END;

CREATE TRIGGER category_versions_no_delete BEFORE DELETE ON category_versions
BEGIN
	SELECT RAISE(ABORT, 'a category version is immutable: DELETE refused');
END;

CREATE TABLE listings (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	trade TEXT NOT NULL CHECK (trade GLOB 'gcid:?*'),
	country TEXT NOT NULL CHECK (country GLOB '[A-Z][A-Z]'),
	author BLOB NOT NULL REFERENCES users (user_id),
	title TEXT NOT NULL CHECK (title = trim(title) AND title != '' AND length(title) <= 100),
	about TEXT NOT NULL CHECK (about = trim(about) AND length(about) <= 4000),
	forked_from INTEGER REFERENCES listings (id),
	-- the standard library's order; curated
	standard_rank INTEGER,
	created_at INTEGER NOT NULL,
	withdrawn_at INTEGER
) STRICT;

CREATE INDEX listings_trade ON listings (trade, country);

CREATE TRIGGER listings_released BEFORE UPDATE ON listings
WHEN NEW.id IS NOT OLD.id OR NEW.trade IS NOT OLD.trade OR NEW.country IS NOT OLD.country OR NEW.author IS NOT OLD.author
	OR NEW.title IS NOT OLD.title OR NEW.about IS NOT OLD.about OR NEW.forked_from IS NOT OLD.forked_from
	OR NEW.created_at IS NOT OLD.created_at OR (OLD.withdrawn_at IS NOT NULL AND NEW.withdrawn_at IS NOT OLD.withdrawn_at)
BEGIN
	SELECT RAISE(ABORT, 'a listing is released for good: only its rank changes, and it is withdrawn once: UPDATE refused');
END;

CREATE TRIGGER listings_no_delete BEFORE DELETE ON listings
BEGIN
	SELECT RAISE(ABORT, 'a listing is released for good: DELETE refused');
END;

CREATE TABLE listing_versions (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	listing INTEGER NOT NULL REFERENCES listings (id),
	n INTEGER NOT NULL CHECK (n >= 1),
	version INTEGER NOT NULL REFERENCES category_versions (id),
	notes TEXT NOT NULL CHECK (notes = trim(notes) AND length(notes) <= 4000),
	published_at INTEGER NOT NULL,
	UNIQUE (listing, n)
) STRICT;

CREATE INDEX listing_versions_version ON listing_versions (version);

CREATE TRIGGER listing_versions_next BEFORE INSERT ON listing_versions
WHEN NEW.n IS NOT (SELECT COALESCE(max(n), 0) + 1 FROM listing_versions WHERE listing = NEW.listing)
	OR (SELECT trade || country FROM category_versions WHERE id = NEW.version) IS NOT (SELECT trade || country FROM listings WHERE id = NEW.listing)
	OR (SELECT withdrawn_at FROM listings WHERE id = NEW.listing) IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'a listing releases its next version, of its own trade and country, until withdrawn: INSERT refused');
END;

CREATE TRIGGER listing_versions_no_update BEFORE UPDATE ON listing_versions
BEGIN
	SELECT RAISE(ABORT, 'a released version is released for good: UPDATE refused');
END;

CREATE TRIGGER listing_versions_no_delete BEFORE DELETE ON listing_versions
BEGIN
	SELECT RAISE(ABORT, 'a released version is released for good: DELETE refused');
END;

CREATE TABLE category_moves (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	owner BLOB NOT NULL REFERENCES users (user_id),
	trade TEXT NOT NULL,
	country TEXT NOT NULL,
	why TEXT NOT NULL CHECK (why IN ('edit', 'adopt', 'update', 'revert')),
	-- none: the category's creation
	from_version INTEGER REFERENCES category_versions (id),
	to_version INTEGER NOT NULL,
	listing_version INTEGER REFERENCES listing_versions (id),
	at INTEGER NOT NULL,
	CHECK (from_version IS NOT to_version),
	CHECK ((why IN ('adopt', 'update')) = (listing_version IS NOT NULL)),
	CHECK (why NOT IN ('update', 'revert') OR from_version IS NOT NULL),
	FOREIGN KEY (to_version, trade, country) REFERENCES category_versions (id, trade, country)
) STRICT;

CREATE INDEX category_moves_category ON category_moves (owner, trade, country, id);

CREATE TRIGGER category_moves_released BEFORE INSERT ON category_moves
WHEN NEW.listing_version IS NOT NULL AND NEW.to_version IS NOT (SELECT version FROM listing_versions WHERE id = NEW.listing_version)
BEGIN
	SELECT RAISE(ABORT, 'an adopt or update moves to the version its listing released: INSERT refused');
END;

CREATE TRIGGER category_moves_no_update BEFORE UPDATE ON category_moves
BEGIN
	SELECT RAISE(ABORT, 'a category''s history is append-only: UPDATE refused');
END;

CREATE TRIGGER category_moves_no_delete BEFORE DELETE ON category_moves
BEGIN
	SELECT RAISE(ABORT, 'a category''s history is append-only: DELETE refused');
END;

CREATE TABLE categories_new (
	owner BLOB NOT NULL REFERENCES users (user_id),
	trade TEXT NOT NULL,
	country TEXT NOT NULL,
	head INTEGER NOT NULL,
	PRIMARY KEY (owner, trade, country),
	FOREIGN KEY (head, trade, country) REFERENCES category_versions (id, trade, country)
) STRICT;

CREATE TABLE published_studies (
	-- placement's Definition::id
	sha TEXT PRIMARY KEY CHECK (length(sha) = 64),
	-- placement's Definition, the area's geometry frozen in it
	definition TEXT NOT NULL CHECK (json_valid(definition) AND json_type(definition) = 'object'),
	listing_version INTEGER NOT NULL REFERENCES listing_versions (id),
	author BLOB NOT NULL REFERENCES users (user_id),
	title TEXT NOT NULL CHECK (title = trim(title) AND title != '' AND length(title) <= 100),
	at INTEGER NOT NULL,
	withdrawn_at INTEGER
) STRICT;

CREATE INDEX published_studies_listing_version ON published_studies (listing_version);

CREATE TRIGGER published_studies_released BEFORE UPDATE ON published_studies
WHEN NEW.sha IS NOT OLD.sha OR NEW.definition IS NOT OLD.definition OR NEW.listing_version IS NOT OLD.listing_version
	OR NEW.author IS NOT OLD.author OR NEW.title IS NOT OLD.title OR NEW.at IS NOT OLD.at OR OLD.withdrawn_at IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'a published study is released for good, and withdrawn once: UPDATE refused');
END;

CREATE TRIGGER published_studies_no_delete BEFORE DELETE ON published_studies
BEGIN
	SELECT RAISE(ABORT, 'a published study is released for good: DELETE refused');
END;

-- the trade picker's Used: listings a person adopted, or whose study they loaded
CREATE TABLE library_seen (
	user BLOB NOT NULL REFERENCES users (user_id),
	listing INTEGER NOT NULL REFERENCES listings (id),
	last_at INTEGER NOT NULL,
	PRIMARY KEY (user, listing)
) STRICT;

INSERT INTO category_versions (trade, country, body, author, at)
SELECT c.google_primary, c.country,
	json_object('keywords', json(c.keywords), 'additional', json(c.google_additional), 'excluded', json(c.excluded), 'demand', json(c.demand)),
	t.owner, t.created_at
FROM categories c JOIN trades t ON t.id = c.trade_id ORDER BY t.id, c.country;

INSERT INTO category_moves (owner, trade, country, why, from_version, to_version, listing_version, at)
SELECT author, trade, country, 'edit', NULL, id, NULL, at FROM category_versions ORDER BY id;

INSERT INTO categories_new (owner, trade, country, head) SELECT author, trade, country, id FROM category_versions;

CREATE TRIGGER categories_moved_on_insert BEFORE INSERT ON categories_new
WHEN NOT EXISTS (SELECT 1 FROM category_moves m WHERE m.owner = NEW.owner AND m.trade = NEW.trade AND m.country = NEW.country
	AND m.to_version = NEW.head AND m.from_version IS NULL
	AND m.id = (SELECT max(id) FROM category_moves s WHERE s.owner = NEW.owner AND s.trade = NEW.trade AND s.country = NEW.country))
BEGIN
	SELECT RAISE(ABORT, 'a category is created by a move onto its first version: INSERT refused');
END;

CREATE TRIGGER categories_moved_on_update BEFORE UPDATE ON categories_new
WHEN NEW.owner IS NOT OLD.owner OR NEW.trade IS NOT OLD.trade OR NEW.country IS NOT OLD.country
	OR NOT EXISTS (SELECT 1 FROM category_moves m WHERE m.owner = NEW.owner AND m.trade = NEW.trade AND m.country = NEW.country
	AND m.from_version IS OLD.head AND m.to_version = NEW.head
	AND m.id = (SELECT max(id) FROM category_moves s WHERE s.owner = NEW.owner AND s.trade = NEW.trade AND s.country = NEW.country))
BEGIN
	SELECT RAISE(ABORT, 'a category''s head moves only by the move its history records last: UPDATE refused');
END;

CREATE TABLE brands_new (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	owner BLOB NOT NULL REFERENCES users (user_id),
	gmail_id INTEGER NOT NULL REFERENCES gmails (id),
	trade TEXT NOT NULL,
	country TEXT NOT NULL,
	name TEXT NOT NULL CHECK (name = trim(name) AND name != '' AND length(name) <= 100),
	site TEXT NOT NULL CHECK (site GLOB '[a-z0-9]*' AND site NOT GLOB '*[^a-z0-9_-]*' AND length(site) <= 64),
	-- PNG
	logo BLOB,
	logo_sha256 BLOB CHECK (length(logo_sha256) = 32),
	created_at INTEGER NOT NULL,
	removed_at INTEGER,
	CHECK ((logo IS NULL) = (logo_sha256 IS NULL)),
	FOREIGN KEY (owner, trade, country) REFERENCES categories (owner, trade, country)
) STRICT;

INSERT INTO brands_new (id, owner, gmail_id, trade, country, name, site, logo, logo_sha256, created_at, removed_at)
SELECT b.id, m.owner, b.gmail_id, c.google_primary, b.country, b.name, b.site, b.logo, b.logo_sha256, b.created_at, b.removed_at
FROM brands b JOIN gmails m ON m.id = b.gmail_id JOIN categories c ON c.trade_id = b.trade_id AND c.country = b.country;

CREATE TABLE kit_drafts_new (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	owner BLOB NOT NULL REFERENCES users (user_id),
	trade TEXT NOT NULL,
	country TEXT NOT NULL,
	text TEXT NOT NULL CHECK (text = trim(text) AND text != '' AND length(text) <= 4000),
	created_at INTEGER NOT NULL,
	removed_at INTEGER,
	FOREIGN KEY (owner, trade, country) REFERENCES categories (owner, trade, country)
) STRICT;

INSERT INTO kit_drafts_new (id, owner, trade, country, text, created_at, removed_at)
SELECT d.id, t.owner, c.google_primary, d.country, d.text, d.created_at, d.removed_at
FROM kit_drafts d JOIN trades t ON t.id = d.trade_id JOIN categories c ON c.trade_id = d.trade_id AND c.country = d.country;

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
	-- The Google category it is meant to have as its primary; google_primary is what Google says.
	trade TEXT CHECK (trade GLOB 'gcid:?*'),
	purged_at INTEGER CHECK (purged_at IS NULL OR removed_at IS NOT NULL),
	website TEXT,
	hours TEXT,
	google_primary TEXT,
	brand_id INTEGER REFERENCES brands (id),
	synced_at INTEGER,
	role TEXT CHECK (role IS NULL OR (role IN ('owner', 'manager') AND gmail_id IS NOT NULL)),
	owner_gmail TEXT CHECK (owner_gmail IS NULL OR (owner_gmail = lower(trim(owner_gmail)) AND owner_gmail != '' AND role = 'manager')),
	CHECK ((lat IS NULL) = (lng IS NULL) AND (placed_by IS NULL OR lat IS NOT NULL)),
	CHECK (country_by IS NULL OR country IS NOT NULL),
	CONSTRAINT a_draft_has_no_place CHECK (removed_at IS NOT NULL OR state != 'draft' OR place_id IS NULL),
	CONSTRAINT a_verified_gbp_has_a_place CHECK (removed_at IS NOT NULL OR state != 'verified' OR place_id IS NOT NULL),
	CONSTRAINT past_draft_a_gbp_has_a_gmail_and_a_country CHECK (removed_at IS NOT NULL OR state = 'draft' OR (gmail_id IS NOT NULL AND country IS NOT NULL))
) STRICT;

INSERT INTO gmbs_new (id, owner, gmail_id, place_id, name, address, state, country, country_by, phone, lat, lng, placed_by, created_at, removed_at,
	areas, hidden_address, customers_visit, film, trade, purged_at, website, hours, google_primary, brand_id, synced_at, role, owner_gmail)
SELECT g.id, g.owner, g.gmail_id, g.place_id, g.name, g.address, g.state, g.country, g.country_by, g.phone, g.lat, g.lng, g.placed_by, g.created_at, g.removed_at,
	g.areas, g.hidden_address, g.customers_visit, g.film, (SELECT t.gcid FROM gmb_trade t WHERE t.gmb = g.id), g.purged_at, g.website, g.hours, g.google_primary,
	g.brand_id, g.synced_at, g.role, g.owner_gmail
FROM gmbs g;

DROP TABLE gmbs;
ALTER TABLE gmbs_new RENAME TO gmbs;
DROP TABLE brands;
ALTER TABLE brands_new RENAME TO brands;
DROP TABLE kit_drafts;
ALTER TABLE kit_drafts_new RENAME TO kit_drafts;
DROP TABLE categories;
DROP TABLE trades;
ALTER TABLE categories_new RENAME TO categories;
DELETE FROM placement_pulls;
DROP TABLE gmb_trade;
DROP TRIGGER refused_stops;
DROP TABLE refused;

-- per category: the listing version its last adopt or update since its creation took it from
CREATE VIEW category_lineage AS
SELECT c.owner, c.trade, c.country, c.head, (
	SELECT m.listing_version FROM category_moves m
	WHERE m.owner = c.owner AND m.trade = c.trade AND m.country = c.country AND m.why IN ('adopt', 'update')
		AND m.id >= (SELECT max(s.id) FROM category_moves s WHERE s.owner = c.owner AND s.trade = c.trade AND s.country = c.country AND s.from_version IS NULL)
	ORDER BY m.id DESC LIMIT 1
) AS listing_version
FROM categories c;

CREATE UNIQUE INDEX gmbs_name ON gmbs (name) WHERE removed_at IS NULL;
CREATE UNIQUE INDEX gmbs_place ON gmbs (place_id) WHERE removed_at IS NULL;
CREATE UNIQUE INDEX gmbs_phone ON gmbs (phone) WHERE removed_at IS NULL;
CREATE INDEX gmbs_owner ON gmbs (owner);
CREATE INDEX gmbs_trade ON gmbs (owner, trade, country);
CREATE INDEX gmbs_brand ON gmbs (brand_id);

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

CREATE TRIGGER gmbs_purged_once BEFORE UPDATE OF purged_at ON gmbs
WHEN OLD.removed_at IS NULL AND NEW.purged_at IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'a live GMB is not purged: UPDATE refused');
END;

CREATE TRIGGER gmbs_brand_on_insert BEFORE INSERT ON gmbs
WHEN NEW.brand_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM brands b WHERE b.id = NEW.brand_id
	AND b.gmail_id IS NEW.gmail_id AND b.trade IS NEW.trade AND b.country IS NEW.country)
BEGIN
	SELECT RAISE(ABORT, 'a GBP''s brand is of its gmail and its category: INSERT refused');
END;

CREATE TRIGGER gmbs_brand_on_update BEFORE UPDATE OF brand_id, gmail_id, trade, country ON gmbs
WHEN NEW.brand_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM brands b WHERE b.id = NEW.brand_id
	AND b.gmail_id IS NEW.gmail_id AND b.trade IS NEW.trade AND b.country IS NEW.country)
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
	AND NEW.film IS OLD.film AND NEW.role IS OLD.role AND NEW.owner_gmail IS OLD.owner_gmail AND NEW.trade IS OLD.trade AND (
		-- its brand given up, so the brand can be removed
		((NEW.brand_id IS NULL OR NEW.brand_id IS OLD.brand_id) AND NEW.removed_at IS OLD.removed_at AND NEW.purged_at IS OLD.purged_at)
		OR (NEW.brand_id IS OLD.brand_id AND OLD.purged_at IS NULL AND (
			(NEW.removed_at IS NULL AND NEW.purged_at IS NULL)
			OR (NEW.removed_at IS OLD.removed_at AND NEW.purged_at IS NOT NULL)))))
BEGIN
	SELECT RAISE(ABORT, 'a removed GMB is restored until purged, purged once, or gives its brand up: UPDATE refused');
END;

CREATE UNIQUE INDEX brands_name ON brands (gmail_id, trade, country, lower(name)) WHERE removed_at IS NULL;
CREATE INDEX brands_category ON brands (owner, trade, country);

CREATE TRIGGER brands_owner BEFORE INSERT ON brands
WHEN NEW.owner IS NOT (SELECT owner FROM gmails WHERE id = NEW.gmail_id)
BEGIN
	SELECT RAISE(ABORT, 'a brand is of its owner''s gmail: INSERT refused');
END;

CREATE TRIGGER brands_fixed BEFORE UPDATE ON brands
WHEN NEW.id IS NOT OLD.id OR NEW.owner IS NOT OLD.owner OR NEW.gmail_id IS NOT OLD.gmail_id OR NEW.trade IS NOT OLD.trade
	OR NEW.country IS NOT OLD.country OR NEW.created_at IS NOT OLD.created_at
BEGIN
	SELECT RAISE(ABORT, 'a brand''s gmail and category never change: UPDATE refused');
END;

CREATE TRIGGER brands_kept BEFORE DELETE ON brands BEGIN SELECT RAISE(ABORT, 'a brand is removed, not deleted'); END;

CREATE INDEX kit_drafts_category ON kit_drafts (owner, trade, country);
CREATE TRIGGER kit_drafts_kept BEFORE DELETE ON kit_drafts BEGIN SELECT RAISE(ABORT, 'a draft is removed, not deleted'); END;

PRAGMA foreign_key_check;

COMMIT;

PRAGMA foreign_keys = ON;
