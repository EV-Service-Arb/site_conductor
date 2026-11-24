-- A trigger, no unique index: a database may hold two already.
CREATE TRIGGER mail_drafts_answered_once BEFORE UPDATE OF state ON mail_drafts
WHEN NEW.state = 'sending' AND OLD.state != 'sending' AND EXISTS (
	SELECT 1 FROM mail_drafts d WHERE d.case_id = NEW.case_id AND d.answers = NEW.answers AND d.id != NEW.id AND d.state IN ('sending', 'sent')
)
BEGIN
	SELECT RAISE(ABORT, 'a message is answered once: UPDATE refused');
END;
