-- Back to Telegram alone: the other destinations, the notes and the inboxes go.

CREATE TABLE alert_fanout (
	rule TEXT NOT NULL,
	event_id BLOB NOT NULL,
	fanned_at INTEGER NOT NULL,
	PRIMARY KEY (rule, event_id)
) STRICT;

INSERT INTO alert_fanout (rule, event_id, fanned_at) SELECT rule, event_id, noted_at FROM alert_notes;
CREATE INDEX alert_fanout_by_time ON alert_fanout (fanned_at);

CREATE TABLE alert_destinations_old (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	user_id BLOB NOT NULL CHECK (length(user_id) = 16),
	method TEXT NOT NULL CHECK (method IN ('telegram')),
	gmail_id INTEGER REFERENCES gmails (id),
	created_at INTEGER NOT NULL
) STRICT;

INSERT INTO alert_destinations_old (id, user_id, method, gmail_id, created_at)
SELECT id, user_id, method, json_extract(filter, '$.gmails[0]'), created_at FROM alert_destinations WHERE method = 'telegram';

CREATE TABLE alert_rules_old (
	destination_id INTEGER NOT NULL REFERENCES alert_destinations_old (id) ON DELETE CASCADE,
	rule TEXT NOT NULL CHECK (rule IN (
		'new_lead', 'contact_overdue', 'payment_received', 'source_silent', 'booked', 'access_requested',
		'review_new', 'review_changed', 'review_gone', 'review_reappeared', 'scan_failed'
	)),
	enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
	PRIMARY KEY (destination_id, rule)
) STRICT;

INSERT INTO alert_rules_old (destination_id, rule, enabled)
SELECT r.destination_id, r.rule, r.cadence != 'off' FROM alert_rules r JOIN alert_destinations_old d ON d.id = r.destination_id
WHERE r.rule IN ('new_lead', 'contact_overdue', 'payment_received', 'source_silent', 'booked', 'access_requested',
	'review_new', 'review_changed', 'review_gone', 'review_reappeared', 'scan_failed');

CREATE TABLE telegram_destinations_old (
	destination_id INTEGER PRIMARY KEY REFERENCES alert_destinations_old (id) ON DELETE CASCADE,
	kind TEXT NOT NULL CHECK (kind IN ('private', 'channel', 'group')),
	chat_id INTEGER NOT NULL,
	topic_id INTEGER CHECK (topic_id IS NULL OR (kind = 'group' AND topic_id > 0)),
	title TEXT,
	dead_at INTEGER,
	dead_reason TEXT,
	CHECK ((dead_at IS NULL) = (dead_reason IS NULL)),
	CHECK ((kind = 'private') = (chat_id > 0))
) STRICT;

INSERT INTO telegram_destinations_old (destination_id, kind, chat_id, topic_id, title, dead_at, dead_reason)
SELECT t.destination_id, t.kind, t.chat_id, t.topic_id, d.title, d.dead_at, d.dead_reason
FROM telegram_destinations t JOIN alert_destinations d ON d.id = t.destination_id;

CREATE TABLE telegram_outbox (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	destination_id INTEGER NOT NULL REFERENCES alert_destinations_old (id) ON DELETE CASCADE,
	user_id BLOB NOT NULL CHECK (length(user_id) = 16),
	chat_id INTEGER NOT NULL,
	topic_id INTEGER,
	rule TEXT NOT NULL,
	event_id BLOB NOT NULL,
	buttons TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(buttons) AND json_type(buttons) = 'array'),
	photo TEXT,
	text_sealed BLOB,
	data_key_fp BLOB CHECK (length(data_key_fp) = 32),
	state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'sent', 'dead')),
	attempts INTEGER NOT NULL DEFAULT 0,
	next_attempt_at INTEGER NOT NULL,
	leased_until INTEGER,
	attempted_at INTEGER,
	sent_at INTEGER,
	tg_message_id INTEGER,
	last_error TEXT,
	created_at INTEGER NOT NULL,
	UNIQUE (rule, event_id, destination_id),
	CHECK ((state = 'pending') = (text_sealed IS NOT NULL)),
	CHECK ((text_sealed IS NULL) = (data_key_fp IS NULL))
) STRICT;

INSERT INTO sqlite_sequence (name, seq) SELECT 'telegram_outbox', seq FROM sqlite_sequence WHERE name = 'alert_outbox';

CREATE INDEX telegram_outbox_due ON telegram_outbox (next_attempt_at, id) WHERE state = 'pending';
CREATE INDEX telegram_outbox_by_attempt ON telegram_outbox (attempted_at);
CREATE INDEX telegram_outbox_by_chat ON telegram_outbox (chat_id, attempted_at);
CREATE INDEX telegram_outbox_by_destination ON telegram_outbox (destination_id, rule) WHERE state = 'pending';

DROP TABLE alert_outbox;
DROP TABLE alert_pauses;
DROP TABLE alert_inbox;
DROP TABLE alert_notes;
DROP TABLE alert_tiers;
DROP TABLE alert_hours;
DROP TABLE telegram_destinations;
DROP TABLE alert_rules;
DROP TABLE alert_destinations;
ALTER TABLE alert_destinations_old RENAME TO alert_destinations;
ALTER TABLE alert_rules_old RENAME TO alert_rules;
ALTER TABLE telegram_destinations_old RENAME TO telegram_destinations;
ALTER TABLE telegram_poller ADD COLUMN outbox_paused_until INTEGER;

CREATE INDEX alert_destinations_by_user ON alert_destinations (user_id);
CREATE UNIQUE INDEX telegram_destinations_by_chat ON telegram_destinations (chat_id, ifnull(topic_id, 0));
