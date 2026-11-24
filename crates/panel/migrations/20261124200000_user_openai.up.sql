-- A person's own OpenAI key (docs/ARCHITECTURE.md, the assistant), sealed under PANEL_DATA_KEY with
-- the user id as associated data. Their answers run on it, after their Claude.
CREATE TABLE user_openai (
	user_id BLOB PRIMARY KEY REFERENCES users (user_id),
	key_sealed BLOB NOT NULL,
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	last4 TEXT NOT NULL CHECK (length(last4) = 4),
	set_at INTEGER NOT NULL,
	checked_at INTEGER NOT NULL,
	-- NULL: OpenAI took it last time.
	check_error TEXT
) STRICT;

-- What a million of GPT-5.6 Terra's tokens list for, prompts up to 272k, cache writes for 30 minutes:
-- https://developers.openai.com/api/docs/pricing, read 2026-11-24. The admin's from here on; without it
-- the item would start at 0, free.
INSERT INTO unit_costs (item, usd_micros, set_by, set_at) VALUES
	('openai.input', 2000000, 'list', 1795478400000000),
	('openai.cache_write', 2500000, 'list', 1795478400000000),
	('openai.cache_read', 200000, 'list', 1795478400000000),
	('openai.output', 12000000, 'list', 1795478400000000);
