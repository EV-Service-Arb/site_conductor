DROP TABLE kit_photo_uses;
DROP TABLE kit_photos;
CREATE TABLE kit_photos (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	trade_id INTEGER NOT NULL,
	country TEXT NOT NULL,
	sha256 BLOB NOT NULL CHECK (length(sha256) = 32),
	bytes BLOB NOT NULL,
	width INTEGER NOT NULL CHECK (width > 0),
	height INTEGER NOT NULL CHECK (height > 0),
	created_at INTEGER NOT NULL,
	removed_at INTEGER,
	FOREIGN KEY (trade_id, country) REFERENCES categories (trade_id, country),
	UNIQUE (trade_id, country, sha256)
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
CREATE TABLE kit_draft_photos (
	draft_id INTEGER NOT NULL REFERENCES kit_drafts (id),
	photo_id INTEGER NOT NULL REFERENCES kit_photos (id),
	pos INTEGER NOT NULL CHECK (pos >= 0),
	PRIMARY KEY (draft_id, photo_id),
	UNIQUE (draft_id, pos)
) STRICT;
CREATE INDEX kit_draft_photos_photo ON kit_draft_photos (photo_id);
CREATE TRIGGER kit_draft_photos_category BEFORE INSERT ON kit_draft_photos
WHEN (SELECT trade_id || ' ' || country FROM kit_drafts WHERE id = NEW.draft_id)
	IS NOT (SELECT trade_id || ' ' || country FROM kit_photos WHERE id = NEW.photo_id)
BEGIN
	SELECT RAISE(ABORT, 'a draft selects photos of its own category');
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
		-- its trade given up, so the trade can be deleted: no service stores a trade id
		(NEW.trade_id IS NULL AND NEW.removed_at IS OLD.removed_at AND NEW.purged_at IS OLD.purged_at)
		OR (NEW.trade_id IS OLD.trade_id AND OLD.purged_at IS NULL AND (
			(NEW.removed_at IS NULL AND NEW.purged_at IS NULL)
			OR (NEW.removed_at IS OLD.removed_at AND NEW.purged_at IS NOT NULL)))))
BEGIN
	SELECT RAISE(ABORT, 'a removed GMB is restored until purged, purged once, or gives its trade up: UPDATE refused');
END;
DROP TRIGGER gmbs_brand_on_update;
DROP TRIGGER gmbs_brand_on_insert;
DROP INDEX gmbs_brand;
ALTER TABLE gmbs DROP COLUMN brand_id;
DROP TABLE brands;
