-- A person's own note on a GBP, read by nobody else (docs/ARCHITECTURE.md, Tokens and GBPs).
CREATE TABLE gmb_notes (
	user_id BLOB NOT NULL REFERENCES users (user_id) ON DELETE CASCADE,
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	note TEXT NOT NULL CHECK (note = trim(note) AND note != '' AND length(note) <= 4000),
	PRIMARY KEY (user_id, gmb_id)
) STRICT;
