CREATE TABLE token_prices (
	item TEXT PRIMARY KEY,
	tokens INTEGER NOT NULL CHECK (tokens >= 0),
	set_by TEXT NOT NULL,
	set_at INTEGER NOT NULL
) STRICT;

INSERT INTO token_prices (item, tokens, set_by, set_at)
SELECT m.item, (c.usd_micros * m.permille + 999999) / 1000000, m.set_by, m.set_at
FROM token_multipliers m
JOIN unit_costs c ON c.id = (SELECT MAX(id) FROM unit_costs WHERE item = m.item)
WHERE c.usd_micros IS NOT NULL;

DROP TABLE token_multipliers;
ALTER TABLE token_ledger DROP COLUMN cost_usd_micros;
