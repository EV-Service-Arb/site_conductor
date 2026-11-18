-- Each item's premium over its cost (sa_core::prices::Rate), the admin's: thousandths, 1000 at cost.
-- Its price is derived: the cost in effect now, in tokens, times this.
CREATE TABLE token_multipliers (
	item TEXT PRIMARY KEY,
	permille INTEGER NOT NULL CHECK (permille >= 0),
	set_by TEXT NOT NULL,
	set_at INTEGER NOT NULL
) STRICT;

CREATE TEMP TABLE priced AS
SELECT p.item, p.tokens, p.set_by, p.set_at,
	(SELECT c.usd_micros FROM unit_costs c WHERE c.item = p.item ORDER BY c.id DESC LIMIT 1) AS cost
FROM token_prices p;

-- what a person pays stays: a price over a known cost is its multiplier, one without a cost is its cost at ×1
INSERT INTO token_multipliers (item, permille, set_by, set_at)
SELECT item, CASE WHEN cost > 0 THEN CAST(round(tokens * 1000000000.0 / (cost * 1000)) AS INTEGER) ELSE 1000 END, set_by, set_at
FROM priced;

INSERT INTO unit_costs (item, usd_micros, set_by, set_at)
SELECT item, tokens * 1000, 'migration: its price, at cost', CAST(unixepoch('subsec') * 1000000 AS INTEGER)
FROM priced WHERE cost IS NULL OR (cost = 0 AND tokens > 0);

DROP TABLE priced;
DROP TABLE token_prices;

-- A charge's cost to us, in millionths of a dollar, at the costs in effect when it was made.
ALTER TABLE token_ledger ADD COLUMN cost_usd_micros INTEGER;
