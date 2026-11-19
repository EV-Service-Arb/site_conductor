-- where the follower of a gmail's inbox stands (docs/ARCHITECTURE.md, Tokens and GBPs): the account it
-- read (another set starts over), its All Mail's UIDVALIDITY, the last UID taken
CREATE TABLE gmail_inbox_cursors (
	gmail_id INTEGER PRIMARY KEY REFERENCES gmails (id),
	username TEXT NOT NULL,
	uid_validity INTEGER NOT NULL,
	last_uid INTEGER NOT NULL
) STRICT;

-- a message is logged once, whichever pass reads it again
CREATE INDEX gmb_events_email ON gmb_events (gmail_id, data ->> 'message') WHERE kind = 'email';
