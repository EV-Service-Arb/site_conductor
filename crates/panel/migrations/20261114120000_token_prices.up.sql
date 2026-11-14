-- Every priced item (sa_core::prices::Item, `<tool>.<variant>`): the admin's price.
CREATE TABLE token_prices (
	item TEXT PRIMARY KEY,
	tokens INTEGER NOT NULL CHECK (tokens >= 0),
	set_by TEXT NOT NULL,
	set_at INTEGER NOT NULL
) STRICT;

INSERT INTO token_prices SELECT 'places.' || sku, tokens, set_by, set_at FROM places_prices;
DROP TABLE places_prices;

-- review_archive's meter, as it charged until its prices became the admin's.
INSERT INTO token_prices (item, tokens, set_by, set_at)
SELECT column1, column2, 'review_archive', CAST(unixepoch('subsec') * 1000000 AS INTEGER)
FROM (VALUES ('walk.open', 7), ('walk.reviews', 1), ('walk.sort', 7), ('walk.sort_retry', 2), ('walk.step', 1));
