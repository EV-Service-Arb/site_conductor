-- The review kit (docs/ARCHITECTURE.md, Review kit): a category's drafts and photos, what went to
-- which GBP, the views that hold drafts while a reviewer reads them, and the areas an operator
-- walks. A use is never undone: a draft or a photo goes to one GBP once.

CREATE TABLE kit_drafts (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	trade_id INTEGER NOT NULL,
	country TEXT NOT NULL,
	text TEXT NOT NULL CHECK (text = trim(text) AND text != '' AND length(text) <= 4000),
	created_at INTEGER NOT NULL,
	removed_at INTEGER,
	FOREIGN KEY (trade_id, country) REFERENCES categories (trade_id, country)
) STRICT;
CREATE INDEX kit_drafts_category ON kit_drafts (trade_id, country);

-- Re-encoded JPEG; sha256 of the bytes as uploaded.
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

-- Absent: the kit is off. review_uri is Places' googleMapsLinks.writeAReviewUri.
CREATE TABLE kit_targets (
	gmb_id INTEGER PRIMARY KEY REFERENCES gmbs (id),
	target TEXT NOT NULL CHECK (target IN ('google', 'drafts', 'one_tap')),
	review_uri TEXT NOT NULL CHECK (review_uri GLOB 'https://*'),
	set_at INTEGER NOT NULL
) STRICT;

-- One load of a reviewer's page. qr: it came from the QR.
CREATE TABLE kit_views (
	id BLOB PRIMARY KEY CHECK (length(id) = 16),
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	qr INTEGER NOT NULL CHECK (qr IN (0, 1)),
	at INTEGER NOT NULL,
	leased_until INTEGER NOT NULL CHECK (leased_until >= at)
) STRICT;
CREATE INDEX kit_views_gmb ON kit_views (gmb_id, leased_until);

CREATE TABLE kit_offered (
	view_id BLOB NOT NULL REFERENCES kit_views (id),
	draft_id INTEGER NOT NULL REFERENCES kit_drafts (id),
	PRIMARY KEY (view_id, draft_id)
) STRICT;
CREATE INDEX kit_offered_draft ON kit_offered (draft_id);

CREATE TABLE kit_uses (
	draft_id INTEGER NOT NULL,
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	view_id BLOB NOT NULL,
	at INTEGER NOT NULL,
	PRIMARY KEY (draft_id, gmb_id),
	FOREIGN KEY (view_id, draft_id) REFERENCES kit_offered (view_id, draft_id)
) STRICT;
CREATE INDEX kit_uses_gmb ON kit_uses (gmb_id, at);

CREATE TABLE kit_photo_uses (
	photo_id INTEGER NOT NULL REFERENCES kit_photos (id),
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	view_id BLOB NOT NULL REFERENCES kit_views (id),
	at INTEGER NOT NULL,
	PRIMARY KEY (photo_id, gmb_id)
) STRICT;

CREATE TRIGGER kit_uses_view_gmb BEFORE INSERT ON kit_uses
WHEN NEW.gmb_id IS NOT (SELECT gmb_id FROM kit_views WHERE id = NEW.view_id)
BEGIN
	SELECT RAISE(ABORT, 'a use is of the GBP its view showed');
END;
CREATE TRIGGER kit_photo_uses_view_gmb BEFORE INSERT ON kit_photo_uses
WHEN NEW.gmb_id IS NOT (SELECT gmb_id FROM kit_views WHERE id = NEW.view_id)
BEGIN
	SELECT RAISE(ABORT, 'a use is of the GBP its view showed');
END;
CREATE TRIGGER kit_uses_kept_update BEFORE UPDATE ON kit_uses BEGIN SELECT RAISE(ABORT, 'a use is kept: UPDATE refused'); END;
CREATE TRIGGER kit_uses_kept_delete BEFORE DELETE ON kit_uses BEGIN SELECT RAISE(ABORT, 'a use is kept: DELETE refused'); END;
CREATE TRIGGER kit_photo_uses_kept_update BEFORE UPDATE ON kit_photo_uses BEGIN SELECT RAISE(ABORT, 'a use is kept: UPDATE refused'); END;
CREATE TRIGGER kit_photo_uses_kept_delete BEFORE DELETE ON kit_photo_uses BEGIN SELECT RAISE(ABORT, 'a use is kept: DELETE refused'); END;
CREATE TRIGGER kit_drafts_kept BEFORE DELETE ON kit_drafts BEGIN SELECT RAISE(ABORT, 'a draft is removed, not deleted'); END;
CREATE TRIGGER kit_photos_kept BEFORE DELETE ON kit_photos BEGIN SELECT RAISE(ABORT, 'a photo is removed, not deleted'); END;

-- The operator showed a GBP's QR.
CREATE TABLE kit_shown (
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	at INTEGER NOT NULL
) STRICT;
CREATE INDEX kit_shown_gmb ON kit_shown (gmb_id, at);

-- Geography only, everyone's: /<slug>/ lists the caller's GBPs in it or next to it.
-- A city is its Google locality's viewport; a circle a centre and a radius.
CREATE TABLE kit_areas (
	slug TEXT PRIMARY KEY CHECK (slug GLOB '[a-z0-9]*' AND slug NOT GLOB '*[^a-z0-9-]*' AND length(slug) <= 64),
	kind TEXT NOT NULL CHECK (kind IN ('city', 'circle')),
	name TEXT NOT NULL CHECK (name = trim(name) AND name != ''),
	place_id TEXT,
	lat REAL NOT NULL CHECK (lat BETWEEN -90 AND 90),
	lng REAL NOT NULL CHECK (lng BETWEEN -180 AND 180),
	radius_m INTEGER CHECK (radius_m > 0),
	south REAL,
	west REAL,
	north REAL,
	east REAL,
	margin_m INTEGER NOT NULL CHECK (margin_m >= 0),
	created_by BLOB NOT NULL REFERENCES users (user_id),
	created_at INTEGER NOT NULL,
	CHECK ((kind = 'city') = (place_id IS NOT NULL AND south IS NOT NULL AND west IS NOT NULL AND north IS NOT NULL AND east IS NOT NULL AND radius_m IS NULL)),
	CHECK ((kind = 'circle') = (place_id IS NULL AND south IS NULL AND west IS NULL AND north IS NULL AND east IS NULL AND radius_m IS NOT NULL)),
	CHECK (south IS NULL OR south <= north)
) STRICT;
