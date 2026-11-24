-- An inbox is two mailboxes of Gmail's, All Mail and Spam, each with its cursor; every one is read again, for what
-- Gmail files among promotions.
CREATE TABLE gmail_inbox_cursors_new (
	gmail_id INTEGER NOT NULL REFERENCES gmails (id),
	spam INTEGER NOT NULL CHECK (spam IN (0, 1)),
	username TEXT NOT NULL,
	uid_validity INTEGER NOT NULL,
	last_uid INTEGER NOT NULL,
	PRIMARY KEY (gmail_id, spam)
) STRICT;

INSERT INTO gmail_inbox_cursors_new SELECT gmail_id, 0, username, uid_validity, 0 FROM gmail_inbox_cursors;
DROP TABLE gmail_inbox_cursors;
ALTER TABLE gmail_inbox_cursors_new RENAME TO gmail_inbox_cursors;

ALTER TABLE mails ADD COLUMN spam INTEGER NOT NULL DEFAULT 0 CHECK (spam IN (0, 1));

-- the case a person made it part of, itself part of none
ALTER TABLE mail_cases ADD COLUMN joined INTEGER REFERENCES mail_cases (id);

-- a dismissed draft may be brought back
DROP TRIGGER mail_drafts_sent_once;
CREATE TRIGGER mail_drafts_sent_once BEFORE UPDATE ON mail_drafts
WHEN (OLD.state = 'sending' AND NEW.state NOT IN ('sending', 'sent', 'failed'))
	OR (OLD.state IN ('sent', 'superseded') AND NEW.state IS NOT OLD.state)
	OR (OLD.state = 'dismissed' AND NEW.state NOT IN ('dismissed', 'proposed'))
BEGIN
	SELECT RAISE(ABORT, 'a draft handed to SMTP is never sent again: UPDATE refused');
END;

-- a message may have been answered in the case it was in before it was joined
DROP TRIGGER mail_drafts_answered_once;
CREATE TRIGGER mail_drafts_answered_once BEFORE UPDATE OF state ON mail_drafts
WHEN NEW.state = 'sending' AND OLD.state != 'sending' AND EXISTS (
	SELECT 1 FROM mail_drafts d WHERE d.answers = NEW.answers AND d.id != NEW.id AND d.state IN ('sending', 'sent')
)
BEGIN
	SELECT RAISE(ABORT, 'a message is answered once: UPDATE refused');
END;
