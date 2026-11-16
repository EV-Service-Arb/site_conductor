-- A study's pull, per Places search it buys; the admin's to change.
INSERT INTO token_prices (item, tokens, set_by, set_at)
VALUES ('placement.search', 8, 'seed', CAST(unixepoch('subsec') * 1000000 AS INTEGER));
