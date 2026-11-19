-- A gmail's mail with Google in its GBPs' logs (`email`, sa_panel::store::gmbs::Event), and where the
-- follower of its inbox stands (docs/ARCHITECTURE.md, Tokens and GBPs). SQLite cannot alter a CHECK:
-- the log is made again with the wider one, every row copied as it is; nothing references it.
DROP TRIGGER gmb_events_no_delete;
DROP TRIGGER gmb_events_no_update;

CREATE TABLE gmb_events_new (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	gmb_id INTEGER REFERENCES gmbs (id),
	-- the gmail it was under when it happened, before the change
	gmail_id INTEGER REFERENCES gmails (id),
	at INTEGER NOT NULL,
	-- none: Google, or a run
	by BLOB REFERENCES users (user_id),
	-- sa_panel::store::gmbs::Event's tag
	kind TEXT NOT NULL CHECK (kind IN ('added', 'state', 'edited', 'synced', 'gmail', 'trade', 'brand', 'film', 'switched', 'removed', 'restored', 'launched', 'stopped', 'email')),
	-- the Event, its kind among it
	data TEXT NOT NULL CHECK (json_valid(data) AND data ->> 'kind' = kind),
	CHECK (gmb_id IS NOT NULL OR gmail_id IS NOT NULL)
) STRICT;

INSERT INTO gmb_events_new (id, gmb_id, gmail_id, at, by, kind, data)
SELECT id, gmb_id, gmail_id, at, by, kind, data FROM gmb_events;

DROP TABLE gmb_events;
ALTER TABLE gmb_events_new RENAME TO gmb_events;

CREATE INDEX gmb_events_gmb ON gmb_events (gmb_id, id);
CREATE INDEX gmb_events_gmail ON gmb_events (gmail_id, id) WHERE gmb_id IS NULL;
-- a message is logged once, whichever pass reads it again
CREATE INDEX gmb_events_email ON gmb_events (gmail_id, data ->> 'message') WHERE kind = 'email';

CREATE TRIGGER gmb_events_no_delete BEFORE DELETE ON gmb_events
BEGIN
	SELECT RAISE(ABORT, 'a GBP''s log is append-only: DELETE refused');
END;

CREATE TRIGGER gmb_events_no_update BEFORE UPDATE ON gmb_events
BEGIN
	SELECT RAISE(ABORT, 'a GBP''s log is append-only: UPDATE refused');
END;

-- the account it read (another set starts over), its All Mail's UIDVALIDITY, the last UID taken
CREATE TABLE gmail_inbox_cursors (
	gmail_id INTEGER PRIMARY KEY REFERENCES gmails (id),
	username TEXT NOT NULL,
	uid_validity INTEGER NOT NULL,
	last_uid INTEGER NOT NULL
) STRICT;
