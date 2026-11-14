CREATE TABLE places_prices (
	sku TEXT PRIMARY KEY CHECK (sku IN ('details', 'search', 'autocomplete')),
	tokens INTEGER NOT NULL CHECK (tokens >= 0),
	set_by TEXT NOT NULL,
	set_at INTEGER NOT NULL
) STRICT;

INSERT INTO places_prices SELECT substr(item, 8), tokens, set_by, set_at FROM token_prices WHERE item LIKE 'places.%';
DROP TABLE token_prices;
