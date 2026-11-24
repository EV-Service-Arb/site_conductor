CREATE TABLE alert_rules_old (
	destination_id INTEGER NOT NULL REFERENCES alert_destinations (id) ON DELETE CASCADE,
	rule TEXT NOT NULL CHECK (rule IN (
		'new_lead', 'contact_overdue', 'payment_received', 'source_silent', 'booked', 'access_requested',
		'review_new', 'review_changed', 'review_gone', 'review_reappeared', 'scan_failed'
	)),
	enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
	PRIMARY KEY (destination_id, rule)
) STRICT;

INSERT INTO alert_rules_old SELECT destination_id, rule, enabled FROM alert_rules WHERE rule != 'google_mail';
DROP TABLE alert_rules;
ALTER TABLE alert_rules_old RENAME TO alert_rules;

DROP TABLE mail_stuck;
DROP TABLE gmail_mail_auto;
DROP TABLE mail_drafts;
DROP TABLE mails;
DROP TABLE mail_cases;
