-- A brand (docs/ARCHITECTURE.md, A GBP's brand): under one gmail and one category of its owner's, the
-- name its GBPs share, its logo and the kit's photos; a GBP holds at most one, of its own gmail and
-- category. `site` is sites' BrandId, which many brands may share.

-- the photos can't be placed under a brand that doesn't exist yet: an empty kit_photos is asked
CREATE TEMP TABLE kit_photos_empty (n INTEGER NOT NULL CHECK (n = 0));
INSERT INTO kit_photos_empty SELECT COUNT(*) FROM kit_photos;
DROP TABLE kit_photos_empty;

CREATE TABLE brands (
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
-- Folds ASCII only: the panel compares the names folded in full before it writes.
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

ALTER TABLE gmbs ADD COLUMN brand_id INTEGER REFERENCES brands (id);
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

DROP TRIGGER gmbs_removed_stays;
CREATE TRIGGER gmbs_removed_stays BEFORE UPDATE ON gmbs
WHEN OLD.removed_at IS NOT NULL AND NOT (
	NEW.id IS OLD.id AND NEW.owner IS OLD.owner AND NEW.gmail_id IS OLD.gmail_id AND NEW.place_id IS OLD.place_id
	AND NEW.name IS OLD.name AND NEW.address IS OLD.address AND NEW.state IS OLD.state AND NEW.country IS OLD.country
	AND NEW.country_by IS OLD.country_by AND NEW.phone IS OLD.phone AND NEW.lat IS OLD.lat AND NEW.lng IS OLD.lng
	AND NEW.placed_by IS OLD.placed_by AND NEW.created_at IS OLD.created_at
	AND NEW.areas IS OLD.areas AND NEW.hidden_address IS OLD.hidden_address AND NEW.customers_visit IS OLD.customers_visit
	AND NEW.film IS OLD.film AND (
		-- its trade or brand given up, so the trade can be deleted and the brand removed
		((NEW.trade_id IS NULL OR NEW.trade_id IS OLD.trade_id) AND (NEW.brand_id IS NULL OR NEW.brand_id IS OLD.brand_id)
			AND NEW.removed_at IS OLD.removed_at AND NEW.purged_at IS OLD.purged_at)
		OR (NEW.trade_id IS OLD.trade_id AND NEW.brand_id IS OLD.brand_id AND OLD.purged_at IS NULL AND (
			(NEW.removed_at IS NULL AND NEW.purged_at IS NULL)
			OR (NEW.removed_at IS OLD.removed_at AND NEW.purged_at IS NOT NULL)))))
BEGIN
	SELECT RAISE(ABORT, 'a removed GMB is restored until purged, purged once, or gives its trade or brand up: UPDATE refused');
END;

-- Drafts stay the category's; photos are a brand's, so no draft selects one.
DROP TABLE kit_draft_photos;
DROP TABLE kit_photo_uses;
DROP TABLE kit_photos;
-- Re-encoded JPEG; sha256 of the bytes as uploaded.
CREATE TABLE kit_photos (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	brand_id INTEGER NOT NULL REFERENCES brands (id),
	sha256 BLOB NOT NULL CHECK (length(sha256) = 32),
	bytes BLOB NOT NULL,
	width INTEGER NOT NULL CHECK (width > 0),
	height INTEGER NOT NULL CHECK (height > 0),
	created_at INTEGER NOT NULL,
	removed_at INTEGER,
	UNIQUE (brand_id, sha256)
) STRICT;
CREATE TABLE kit_photo_uses (
	photo_id INTEGER NOT NULL REFERENCES kit_photos (id),
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	view_id BLOB NOT NULL REFERENCES kit_views (id),
	at INTEGER NOT NULL,
	PRIMARY KEY (photo_id, gmb_id)
) STRICT;
CREATE TRIGGER kit_photo_uses_view_gmb BEFORE INSERT ON kit_photo_uses
WHEN NEW.gmb_id IS NOT (SELECT gmb_id FROM kit_views WHERE id = NEW.view_id)
BEGIN
	SELECT RAISE(ABORT, 'a use is of the GBP its view showed');
END;
CREATE TRIGGER kit_photo_uses_kept_update BEFORE UPDATE ON kit_photo_uses BEGIN SELECT RAISE(ABORT, 'a use is kept: UPDATE refused'); END;
CREATE TRIGGER kit_photo_uses_kept_delete BEFORE DELETE ON kit_photo_uses BEGIN SELECT RAISE(ABORT, 'a use is kept: DELETE refused'); END;
CREATE TRIGGER kit_photos_kept BEFORE DELETE ON kit_photos BEGIN SELECT RAISE(ABORT, 'a photo is removed, not deleted'); END;
