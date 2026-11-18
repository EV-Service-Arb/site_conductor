-- A GBP's log: what changed of it and what was done with it, in the write that did it. A gmail's own
-- (its browser run for the gmail alone) has no gmb_id, and is each of its GBPs'.
CREATE TABLE gmb_events (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	gmb_id INTEGER REFERENCES gmbs (id),
	-- the gmail it was under when it happened, before the change
	gmail_id INTEGER REFERENCES gmails (id),
	at INTEGER NOT NULL,
	-- none: Google, or a run
	by BLOB REFERENCES users (user_id),
	-- sa_panel::store::gmbs::Event's tag
	kind TEXT NOT NULL CHECK (kind IN ('added', 'state', 'edited', 'synced', 'gmail', 'trade', 'brand', 'film', 'switched', 'removed', 'restored', 'launched', 'stopped')),
	-- the Event, its kind among it
	data TEXT NOT NULL CHECK (json_valid(data) AND data ->> 'kind' = kind),
	CHECK (gmb_id IS NOT NULL OR gmail_id IS NOT NULL)
) STRICT;
CREATE INDEX gmb_events_gmb ON gmb_events (gmb_id, id);
CREATE INDEX gmb_events_gmail ON gmb_events (gmail_id, id) WHERE gmb_id IS NULL;

CREATE TRIGGER gmb_events_no_delete BEFORE DELETE ON gmb_events
BEGIN
	SELECT RAISE(ABORT, 'a GBP''s log is append-only: DELETE refused');
END;

CREATE TRIGGER gmb_events_no_update BEFORE UPDATE ON gmb_events
BEGIN
	SELECT RAISE(ABORT, 'a GBP''s log is append-only: UPDATE refused');
END;

-- when a Sync last wrote it
ALTER TABLE gmbs ADD COLUMN synced_at INTEGER;
