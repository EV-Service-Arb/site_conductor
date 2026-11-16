CREATE TABLE unit_costs_old (
	item TEXT PRIMARY KEY,
	usd_micros INTEGER NOT NULL CHECK (usd_micros >= 0),
	set_by TEXT NOT NULL,
	set_at INTEGER NOT NULL
) STRICT;
INSERT INTO unit_costs_old
SELECT item, usd_micros, set_by, set_at FROM unit_costs c
WHERE id = (SELECT MAX(id) FROM unit_costs WHERE item = c.item) AND usd_micros IS NOT NULL;
DROP TABLE unit_costs;
ALTER TABLE unit_costs_old RENAME TO unit_costs;
