-- Alerts: what happened kept as notes, read in the panel (the inbox); a destination reached by any
-- method, told of each tier at its own cadence, for what its filter takes; one outbox for them all.

-- A note, whoever it is for; the rows it fans out to name it.
CREATE TABLE alert_notes (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	rule TEXT NOT NULL,
	event_id BLOB NOT NULL CHECK (length(event_id) = 16),
	-- Its own; NULL: its rule's.
	tier TEXT CHECK (tier IN ('info', 'important', 'urgent')),
	-- Its text, and the one for who may see PII.
	body_sealed BLOB NOT NULL,
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	gmbs TEXT NOT NULL CHECK (json_valid(gmbs) AND json_type(gmbs) = 'array'),
	-- The gmail it is about, when it is about no GMB of it.
	gmail_id INTEGER REFERENCES gmails (id),
	-- A path on the conductor's origin.
	link TEXT CHECK (link GLOB '/*' AND link NOT GLOB '//*'),
	-- A path on the rule's service: the photo, fetched under the conductor's assertion at the send.
	photo TEXT,
	buttons TEXT NOT NULL CHECK (json_valid(buttons) AND json_type(buttons) = 'array'),
	noted_at INTEGER NOT NULL,
	UNIQUE (rule, event_id)
) STRICT;

CREATE INDEX alert_notes_by_time ON alert_notes (noted_at);

-- A person's note in the panel.
CREATE TABLE alert_inbox (
	user_id BLOB NOT NULL CHECK (length(user_id) = 16),
	note_id INTEGER NOT NULL REFERENCES alert_notes (id) ON DELETE CASCADE,
	-- As it was for them when it came.
	tier TEXT NOT NULL CHECK (tier IN ('info', 'important', 'urgent')),
	read_at INTEGER,
	-- The last of its buttons they pressed.
	pressed TEXT,
	PRIMARY KEY (user_id, note_id)
) STRICT;

CREATE INDEX alert_inbox_by_note ON alert_inbox (note_id);
CREATE INDEX alert_inbox_unread ON alert_inbox (user_id, tier) WHERE read_at IS NULL;

-- A rule's tier as a person set it.
CREATE TABLE alert_tiers (
	user_id BLOB NOT NULL CHECK (length(user_id) = 16),
	rule TEXT NOT NULL,
	tier TEXT NOT NULL CHECK (tier IN ('info', 'important', 'urgent')),
	PRIMARY KEY (user_id, rule)
) STRICT;

-- A person's hours; minutes since midnight in their time zone.
CREATE TABLE alert_hours (
	user_id BLOB PRIMARY KEY CHECK (length(user_id) = 16),
	tz TEXT NOT NULL,
	daily INTEGER NOT NULL CHECK (daily BETWEEN 0 AND 1439),
	quiet_from INTEGER CHECK (quiet_from BETWEEN 0 AND 1439),
	quiet_to INTEGER CHECK (quiet_to BETWEEN 0 AND 1439),
	snoozed_until INTEGER,
	CHECK ((quiet_from IS NULL) = (quiet_to IS NULL)),
	CHECK (quiet_from <> quiet_to)
) STRICT;

CREATE TABLE alert_destinations_new (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	user_id BLOB NOT NULL CHECK (length(user_id) = 16),
	method TEXT NOT NULL CHECK (method IN ('telegram', 'email', 'discord', 'slack', 'push', 'webhook')),
	-- Read by its person alone: the only ones PII and buttons go to.
	private INTEGER NOT NULL CHECK (private IN (0, 1)),
	-- A chat's account or title, an address, a browser: what its person knows it by.
	title TEXT,
	urgent TEXT NOT NULL CHECK (urgent IN ('now', 'unread', 'hourly', 'daily', 'off')),
	important TEXT NOT NULL CHECK (important IN ('now', 'unread', 'hourly', 'daily', 'off')),
	info TEXT NOT NULL CHECK (info IN ('now', 'unread', 'hourly', 'daily', 'off')),
	-- Which notes it is for (sa_core::alerts::Filter).
	filter TEXT NOT NULL CHECK (json_valid(filter) AND json_type(filter) = 'object'),
	-- Its address, but Telegram's (telegram_destinations), and what tells it from the person's others.
	address_sealed BLOB,
	data_key_fp BLOB CHECK (length(data_key_fp) = 32),
	address_tag BLOB CHECK (length(address_tag) = 32),
	-- Nothing goes there until a send succeeds again (a link, a test).
	dead_at INTEGER,
	dead_reason TEXT,
	created_at INTEGER NOT NULL,
	CHECK ((dead_at IS NULL) = (dead_reason IS NULL)),
	CHECK ((method = 'telegram') = (address_sealed IS NULL)),
	CHECK ((address_sealed IS NULL) = (data_key_fp IS NULL) AND (address_sealed IS NULL) = (address_tag IS NULL))
) STRICT;

INSERT INTO alert_destinations_new (id, user_id, method, private, title, urgent, important, info, filter, dead_at, dead_reason, created_at)
SELECT d.id, d.user_id, 'telegram', t.kind = 'private', t.title, 'now', 'now', 'off',
	json_object('gmails', CASE WHEN d.gmail_id IS NULL THEN json('[]') ELSE json_array(d.gmail_id) END, 'trades', json('[]'), 'brands', json('[]'), 'gmbs', json('[]')),
	t.dead_at, t.dead_reason, d.created_at
FROM alert_destinations d JOIN telegram_destinations t ON t.destination_id = d.id;

-- A rule's cadence at a destination, where it is not its tier's there.
CREATE TABLE alert_rules_new (
	destination_id INTEGER NOT NULL REFERENCES alert_destinations_new (id) ON DELETE CASCADE,
	rule TEXT NOT NULL,
	cadence TEXT NOT NULL CHECK (cadence IN ('now', 'unread', 'hourly', 'daily', 'off')),
	PRIMARY KEY (destination_id, rule)
) STRICT;

INSERT INTO alert_rules_new (destination_id, rule, cadence)
SELECT r.destination_id, r.rule, CASE r.enabled WHEN 1 THEN 'now' ELSE 'off' END FROM alert_rules r JOIN alert_destinations_new d ON d.id = r.destination_id;

-- A rule off by default until now stays off where its person did not turn it on.
INSERT INTO alert_rules_new (destination_id, rule, cadence)
SELECT d.id, 'source_silent', 'off' FROM alert_destinations_new d WHERE true ON CONFLICT DO NOTHING;

CREATE TABLE telegram_destinations_new (
	destination_id INTEGER PRIMARY KEY REFERENCES alert_destinations_new (id) ON DELETE CASCADE,
	kind TEXT NOT NULL CHECK (kind IN ('private', 'channel', 'group')),
	chat_id INTEGER NOT NULL,
	topic_id INTEGER CHECK (topic_id IS NULL OR (kind = 'group' AND topic_id > 0)),
	CHECK ((kind = 'private') = (chat_id > 0))
) STRICT;

INSERT INTO telegram_destinations_new (destination_id, kind, chat_id, topic_id) SELECT destination_id, kind, chat_id, topic_id FROM telegram_destinations;

CREATE TABLE alert_outbox (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	destination_id INTEGER NOT NULL REFERENCES alert_destinations_new (id) ON DELETE CASCADE,
	user_id BLOB NOT NULL CHECK (length(user_id) = 16),
	note_id INTEGER NOT NULL REFERENCES alert_notes (id) ON DELETE CASCADE,
	rule TEXT NOT NULL,
	tier TEXT NOT NULL CHECK (tier IN ('info', 'important', 'urgent')),
	-- Goes out as one message with whatever else is due at its destination then.
	together INTEGER NOT NULL CHECK (together IN (0, 1)),
	-- What is paced together: a Telegram chat's id, else the destination.
	lane TEXT NOT NULL,
	state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'sent', 'dead', 'dropped')),
	attempts INTEGER NOT NULL DEFAULT 0,
	next_attempt_at INTEGER NOT NULL,
	leased_until INTEGER,
	attempted_at INTEGER,
	sent_at INTEGER,
	-- What its network calls the message.
	remote_id TEXT,
	last_error TEXT,
	created_at INTEGER NOT NULL,
	UNIQUE (note_id, destination_id)
) STRICT;

-- A button's data names its message's id: one handed out before is never handed out again.
INSERT INTO sqlite_sequence (name, seq) SELECT 'alert_outbox', seq FROM sqlite_sequence WHERE name = 'telegram_outbox';

CREATE INDEX alert_outbox_due ON alert_outbox (next_attempt_at, id) WHERE state = 'pending';
CREATE INDEX alert_outbox_by_attempt ON alert_outbox (attempted_at);
CREATE INDEX alert_outbox_by_destination ON alert_outbox (destination_id, rule) WHERE state = 'pending';
CREATE INDEX alert_outbox_by_user ON alert_outbox (user_id, note_id) WHERE state = 'pending';

-- A method told to wait (429): nothing of it leaves the outbox before this.
CREATE TABLE alert_pauses (
	method TEXT PRIMARY KEY,
	until INTEGER NOT NULL
) STRICT;

DROP TABLE telegram_outbox;
DROP TABLE telegram_destinations;
DROP TABLE alert_rules;
DROP TABLE alert_destinations;
DROP TABLE alert_fanout;
ALTER TABLE alert_destinations_new RENAME TO alert_destinations;
ALTER TABLE alert_rules_new RENAME TO alert_rules;
ALTER TABLE telegram_destinations_new RENAME TO telegram_destinations;
ALTER TABLE telegram_poller DROP COLUMN outbox_paused_until;

CREATE INDEX alert_destinations_by_user ON alert_destinations (user_id);
CREATE UNIQUE INDEX alert_destinations_by_address ON alert_destinations (user_id, method, address_tag) WHERE address_tag IS NOT NULL;
CREATE UNIQUE INDEX telegram_destinations_by_chat ON telegram_destinations (chat_id, ifnull(topic_id, 0));
