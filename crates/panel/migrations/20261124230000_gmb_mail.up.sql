-- replies of a GBP on a topic go by themselves: a row is on. A gmail's switch was every GBP's under it: each is off until its own.
CREATE TABLE gmb_mail_auto (
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	topic TEXT NOT NULL CHECK (topic IN ('verification', 'reviews')),
	PRIMARY KEY (gmb_id, topic)
) STRICT;

UPDATE mail_drafts SET state = 'proposed' WHERE state = 'held';
DROP TABLE gmail_mail_auto;

CREATE TRIGGER gmb_mail_auto_moved AFTER UPDATE OF gmail_id ON gmbs WHEN NEW.gmail_id IS NOT OLD.gmail_id
BEGIN
	DELETE FROM gmb_mail_auto WHERE gmb_id = NEW.id;
END;

-- what a person gave for a GBP (a Kbis, a bill, a photo of its sign), kept for Google's next ask
CREATE TABLE gmb_documents (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
	kind TEXT NOT NULL,
	size INTEGER NOT NULL CHECK (size > 0),
	bytes_sealed BLOB NOT NULL,
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	-- what Google asked that it answers, in its words
	given_for TEXT,
	added_by BLOB NOT NULL REFERENCES users (user_id),
	added_at INTEGER NOT NULL
) STRICT;

CREATE INDEX gmb_documents_by_gmb ON gmb_documents (gmb_id, id);

-- what a case asks of its person and they have given, by the document or otherwise
CREATE TABLE mail_given (
	case_id INTEGER NOT NULL REFERENCES mail_cases (id),
	item TEXT NOT NULL,
	document_id INTEGER REFERENCES gmb_documents (id) ON DELETE CASCADE,
	given_by BLOB NOT NULL REFERENCES users (user_id),
	given_at INTEGER NOT NULL,
	PRIMARY KEY (case_id, item)
) STRICT;
