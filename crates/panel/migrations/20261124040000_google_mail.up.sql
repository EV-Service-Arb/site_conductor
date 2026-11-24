-- Google's mail about the GBPs, as kept (docs/ARCHITECTURE.md, Google's mail): a case is a conversation
-- or a lone notice of one gmail's.
CREATE TABLE mail_cases (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	gmail_id INTEGER NOT NULL REFERENCES gmails (id),
	-- Google's case numbers its subjects carried
	numbers TEXT NOT NULL CHECK (json_valid(numbers) AND json_type(numbers) = 'array'),
	-- by hand, over what its messages say
	gmb_id INTEGER REFERENCES gmbs (id),
	topic TEXT CHECK (topic IN ('verification', 'suspension', 'reviews', 'activity', 'access', 'profile', 'other')),
	-- its last message when a person marked it done
	done_mail INTEGER REFERENCES mails (id) DEFERRABLE INITIALLY DEFERRED,
	done_by BLOB REFERENCES users (user_id),
	CHECK ((done_mail IS NULL) = (done_by IS NULL))
) STRICT;

CREATE INDEX mail_cases_by_gmail ON mail_cases (gmail_id);

CREATE TABLE mails (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	case_id INTEGER NOT NULL REFERENCES mail_cases (id),
	gmail_id INTEGER NOT NULL REFERENCES gmails (id),
	-- Gmail's X-GM-MSGID, and its X-GM-THRID in hex
	message TEXT NOT NULL,
	thread TEXT NOT NULL,
	at INTEGER NOT NULL,
	sent INTEGER NOT NULL CHECK (sent IN (0, 1)),
	from_address TEXT,
	reply_to TEXT,
	recipients TEXT NOT NULL CHECK (json_valid(recipients) AND json_type(recipients) = 'array'),
	subject TEXT,
	message_id TEXT,
	refs TEXT NOT NULL CHECK (json_valid(refs) AND json_type(refs) = 'array'),
	authentic INTEGER NOT NULL CHECK (authentic IN (0, 1)),
	repliable INTEGER NOT NULL CHECK (repliable IN (0, 1)),
	-- the one GBP of the gmail's its text names, the one profile it links
	gmb_id INTEGER REFERENCES gmbs (id),
	listing TEXT,
	text_sealed BLOB NOT NULL,
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	reading TEXT CHECK (json_valid(reading)),
	read_by TEXT CHECK (read_by IN ('rule', 'model', 'person')),
	read_at INTEGER,
	attempts INTEGER NOT NULL DEFAULT 0,
	read_error TEXT,
	UNIQUE (gmail_id, message),
	CHECK ((reading IS NULL) = (read_by IS NULL)),
	CHECK ((reading IS NULL) = (read_at IS NULL)),
	CHECK (NOT (sent AND repliable)),
	CHECK (authentic OR NOT repliable)
) STRICT;

CREATE INDEX mails_by_case ON mails (case_id, at);

CREATE TABLE mail_drafts (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	case_id INTEGER NOT NULL REFERENCES mail_cases (id),
	-- the case's last message when it was written
	answers INTEGER NOT NULL REFERENCES mails (id),
	body_sealed BLOB NOT NULL,
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	needs TEXT NOT NULL CHECK (json_valid(needs) AND json_type(needs) = 'array'),
	state TEXT NOT NULL CHECK (state IN ('proposed', 'held', 'sending', 'sent', 'dismissed', 'superseded', 'failed')),
	send_at INTEGER,
	created_at INTEGER NOT NULL,
	decided_by BLOB REFERENCES users (user_id),
	sent_at INTEGER,
	message_id TEXT,
	error TEXT,
	CHECK (state NOT IN ('held', 'sending', 'sent') OR send_at IS NOT NULL),
	CHECK ((state = 'sent') = (sent_at IS NOT NULL)),
	CHECK (state NOT IN ('sending', 'sent') OR message_id IS NOT NULL),
	CHECK (state != 'failed' OR error IS NOT NULL)
) STRICT;

CREATE INDEX mail_drafts_by_case ON mail_drafts (case_id, id);
CREATE INDEX mail_drafts_held ON mail_drafts (send_at) WHERE state = 'held';

CREATE TRIGGER mail_drafts_sent_once BEFORE UPDATE ON mail_drafts
WHEN (OLD.state = 'sending' AND NEW.state NOT IN ('sending', 'sent', 'failed'))
	OR (OLD.state IN ('sent', 'dismissed', 'superseded') AND NEW.state IS NOT OLD.state)
BEGIN
	SELECT RAISE(ABORT, 'a draft handed to SMTP is never sent again: UPDATE refused');
END;

-- replies of a topic go by themselves: a row is on
CREATE TABLE gmail_mail_auto (
	gmail_id INTEGER NOT NULL REFERENCES gmails (id),
	topic TEXT NOT NULL CHECK (topic IN ('verification', 'reviews')),
	PRIMARY KEY (gmail_id, topic)
) STRICT;

-- why a person's mail is neither read nor answered: no Claude, no balance
CREATE TABLE mail_stuck (
	user_id BLOB PRIMARY KEY REFERENCES users (user_id) ON DELETE CASCADE,
	why TEXT NOT NULL
) STRICT;

-- every inbox is read again, for the mail kept here; what its log holds is not logged twice
DELETE FROM gmail_inbox_cursors;

-- The rule `google_mail`. SQLite cannot alter a CHECK: the table is made again with the wider one.
CREATE TABLE alert_rules_new (
	destination_id INTEGER NOT NULL REFERENCES alert_destinations (id) ON DELETE CASCADE,
	rule TEXT NOT NULL CHECK (rule IN (
		'new_lead', 'contact_overdue', 'payment_received', 'source_silent', 'booked', 'access_requested',
		'review_new', 'review_changed', 'review_gone', 'review_reappeared', 'scan_failed', 'google_mail'
	)),
	enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
	PRIMARY KEY (destination_id, rule)
) STRICT;

INSERT INTO alert_rules_new SELECT destination_id, rule, enabled FROM alert_rules;
DROP TABLE alert_rules;
ALTER TABLE alert_rules_new RENAME TO alert_rules;
