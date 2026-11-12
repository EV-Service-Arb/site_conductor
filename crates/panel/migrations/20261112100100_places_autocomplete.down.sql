CREATE TABLE places_prices_new (
	sku TEXT PRIMARY KEY CHECK (sku IN ('details', 'search')),
	tokens INTEGER NOT NULL CHECK (tokens >= 0),
	set_by TEXT NOT NULL,
	set_at INTEGER NOT NULL
) STRICT;

INSERT INTO places_prices_new SELECT sku, tokens, set_by, set_at FROM places_prices WHERE sku != 'autocomplete';
DROP TABLE places_prices;
ALTER TABLE places_prices_new RENAME TO places_prices;
