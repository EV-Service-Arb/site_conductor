-- A kit is a named selection of an owner's GBPs to get reviews for; it owns no content: drafts stay
-- their category's, photos their brand's. Its area is a reference only, membership is its rows.
CREATE TABLE kits (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	owner BLOB NOT NULL REFERENCES users (user_id),
	slug TEXT NOT NULL CHECK (slug GLOB '[a-z0-9]*' AND slug NOT GLOB '*[^a-z0-9-]*' AND length(slug) <= 64 AND slug != 'new'),
	name TEXT NOT NULL CHECK (name = trim(name) AND name != '' AND length(name) <= 80),
	area TEXT REFERENCES areas (slug),
	created_at INTEGER NOT NULL,
	UNIQUE (owner, slug)
) STRICT;

CREATE TABLE kit_members (
	kit_id INTEGER NOT NULL REFERENCES kits (id) ON DELETE CASCADE,
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	added_at INTEGER NOT NULL,
	PRIMARY KEY (kit_id, gmb_id)
) STRICT;
CREATE INDEX kit_members_gmb ON kit_members (gmb_id);

-- None: offered everywhere; else only to a GBP whose point lies in one of the areas.
CREATE TABLE kit_draft_places (
	draft_id INTEGER NOT NULL REFERENCES kit_drafts (id),
	area TEXT NOT NULL REFERENCES areas (slug),
	PRIMARY KEY (draft_id, area)
) STRICT;

-- What a brand's photo shows, for the reviewer; NULL: not labelled yet.
ALTER TABLE kit_photos ADD COLUMN shows TEXT CHECK (shows = trim(shows) AND shows != '' AND length(shows) <= 60);

-- review_archive's reviews of our GBPs (feeds::REVIEWS). draft_id: the copy it was matched to,
-- decided once when it was first taken.
CREATE TABLE kit_posted (
	review_id TEXT PRIMARY KEY,
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	rating INTEGER NOT NULL CHECK (rating BETWEEN 1 AND 5),
	first_seen INTEGER NOT NULL,
	gone_at INTEGER,
	draft_id INTEGER REFERENCES kit_drafts (id),
	UNIQUE (gmb_id, draft_id)
) STRICT;
CREATE INDEX kit_posted_gmb ON kit_posted (gmb_id, first_seen);

CREATE TABLE kit_posted_cursor (
	id INTEGER PRIMARY KEY CHECK (id = 1),
	after INTEGER NOT NULL CHECK (after >= 0)
) STRICT;
INSERT INTO kit_posted_cursor (id, after) VALUES (1, 0);
CREATE TRIGGER kit_posted_cursor_forward BEFORE UPDATE ON kit_posted_cursor
WHEN NEW.after < OLD.after
BEGIN
	SELECT RAISE(ABORT, 'the reviews cursor only moves forward: UPDATE refused');
END;
