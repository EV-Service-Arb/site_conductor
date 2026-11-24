DROP TABLE mail_given;
DROP TABLE gmb_documents;
DROP TRIGGER gmb_mail_auto_moved;
UPDATE mail_drafts SET state = 'proposed' WHERE state = 'held';
DROP TABLE gmb_mail_auto;

CREATE TABLE gmail_mail_auto (
	gmail_id INTEGER NOT NULL REFERENCES gmails (id),
	topic TEXT NOT NULL CHECK (topic IN ('verification', 'reviews')),
	PRIMARY KEY (gmail_id, topic)
) STRICT;
