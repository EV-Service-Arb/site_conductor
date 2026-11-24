DROP TRIGGER mail_drafts_answered_once;
CREATE TRIGGER mail_drafts_answered_once BEFORE UPDATE OF state ON mail_drafts
WHEN NEW.state = 'sending' AND OLD.state != 'sending' AND EXISTS (
	SELECT 1 FROM mail_drafts d WHERE d.case_id = NEW.case_id AND d.answers = NEW.answers AND d.id != NEW.id AND d.state IN ('sending', 'sent')
)
BEGIN
	SELECT RAISE(ABORT, 'a message is answered once: UPDATE refused');
END;

DROP TRIGGER mail_drafts_sent_once;
CREATE TRIGGER mail_drafts_sent_once BEFORE UPDATE ON mail_drafts
WHEN (OLD.state = 'sending' AND NEW.state NOT IN ('sending', 'sent', 'failed'))
	OR (OLD.state IN ('sent', 'dismissed', 'superseded') AND NEW.state IS NOT OLD.state)
BEGIN
	SELECT RAISE(ABORT, 'a draft handed to SMTP is never sent again: UPDATE refused');
END;

ALTER TABLE mail_cases DROP COLUMN joined;
ALTER TABLE mails DROP COLUMN spam;

DROP TABLE gmail_inbox_cursors;
CREATE TABLE gmail_inbox_cursors (
	gmail_id INTEGER PRIMARY KEY REFERENCES gmails (id),
	username TEXT NOT NULL,
	uid_validity INTEGER NOT NULL,
	last_uid INTEGER NOT NULL
) STRICT;
