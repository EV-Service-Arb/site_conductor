ALTER TABLE kit_views DROP COLUMN kit_id;
ALTER TABLE kit_shown DROP COLUMN kit_id;
DROP TRIGGER kits_kept;
DROP INDEX kits_slug;
DELETE FROM kit_members WHERE kit_id IN (SELECT id FROM kits WHERE removed_at IS NOT NULL);
CREATE TABLE kits_old (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	owner BLOB NOT NULL REFERENCES users (user_id),
	slug TEXT NOT NULL CHECK (slug GLOB '[a-z0-9]*' AND slug NOT GLOB '*[^a-z0-9-]*' AND length(slug) <= 64 AND slug != 'new'),
	name TEXT NOT NULL CHECK (name = trim(name) AND name != '' AND length(name) <= 80),
	area TEXT REFERENCES areas (slug),
	created_at INTEGER NOT NULL,
	UNIQUE (owner, slug)
) STRICT;
INSERT INTO kits_old (id, owner, slug, name, area, created_at) SELECT id, owner, slug, name, area, created_at FROM kits WHERE removed_at IS NULL;
CREATE TABLE kit_members_old (
	kit_id INTEGER NOT NULL REFERENCES kits_old (id) ON DELETE CASCADE,
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	added_at INTEGER NOT NULL,
	PRIMARY KEY (kit_id, gmb_id)
) STRICT;
INSERT INTO kit_members_old SELECT kit_id, gmb_id, added_at FROM kit_members;
DROP TABLE kit_members;
DROP TABLE kits;
ALTER TABLE kits_old RENAME TO kits;
ALTER TABLE kit_members_old RENAME TO kit_members;
CREATE INDEX kit_members_gmb ON kit_members (gmb_id);
