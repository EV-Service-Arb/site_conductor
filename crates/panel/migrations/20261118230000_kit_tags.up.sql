-- Every kit event carries the kit it came from, so an event counted in two kits' tallies is still one
-- event: a QR shown from a kit's page, a page opened from a kit's QR; a copy, a save and a posted
-- review through their view. A kit is removed, never deleted, so the tags keep pointing at it; its
-- slug is free again once removed.
CREATE TABLE kits_new (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	owner BLOB NOT NULL REFERENCES users (user_id),
	slug TEXT NOT NULL CHECK (slug GLOB '[a-z0-9]*' AND slug NOT GLOB '*[^a-z0-9-]*' AND length(slug) <= 64 AND slug != 'new'),
	name TEXT NOT NULL CHECK (name = trim(name) AND name != '' AND length(name) <= 80),
	area TEXT REFERENCES areas (slug),
	created_at INTEGER NOT NULL,
	removed_at INTEGER
) STRICT;
INSERT INTO kits_new (id, owner, slug, name, area, created_at) SELECT id, owner, slug, name, area, created_at FROM kits;

CREATE TABLE kit_members_new (
	kit_id INTEGER NOT NULL REFERENCES kits_new (id),
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	added_at INTEGER NOT NULL,
	PRIMARY KEY (kit_id, gmb_id)
) STRICT;
INSERT INTO kit_members_new SELECT kit_id, gmb_id, added_at FROM kit_members;

DROP TABLE kit_members;
DROP TABLE kits;
ALTER TABLE kits_new RENAME TO kits;
ALTER TABLE kit_members_new RENAME TO kit_members;
CREATE UNIQUE INDEX kits_slug ON kits (owner, slug) WHERE removed_at IS NULL;
CREATE INDEX kit_members_gmb ON kit_members (gmb_id);
CREATE TRIGGER kits_kept BEFORE DELETE ON kits BEGIN SELECT RAISE(ABORT, 'a kit is removed, not deleted'); END;

ALTER TABLE kit_shown ADD COLUMN kit_id INTEGER REFERENCES kits (id);
ALTER TABLE kit_views ADD COLUMN kit_id INTEGER REFERENCES kits (id);
