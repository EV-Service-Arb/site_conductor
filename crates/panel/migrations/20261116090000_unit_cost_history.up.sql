-- Every cost an admin set, never overwritten: a use is costed at the one in effect when it was done.
CREATE TABLE unit_costs_new (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	item TEXT NOT NULL,
	-- NULL: back to unknown from `set_at`
	usd_micros INTEGER CHECK (usd_micros >= 0),
	set_by TEXT NOT NULL,
	set_at INTEGER NOT NULL
) STRICT;

INSERT INTO unit_costs_new (item, usd_micros, set_by, set_at) SELECT item, usd_micros, set_by, set_at FROM unit_costs ORDER BY set_at;
DROP TABLE unit_costs;
ALTER TABLE unit_costs_new RENAME TO unit_costs;

CREATE INDEX unit_costs_item_at ON unit_costs (item, set_at);

CREATE TRIGGER unit_costs_no_update BEFORE UPDATE ON unit_costs
BEGIN
	SELECT RAISE(ABORT, 'unit costs are append-only: UPDATE refused');
END;

CREATE TRIGGER unit_costs_no_delete BEFORE DELETE ON unit_costs
BEGIN
	SELECT RAISE(ABORT, 'unit costs are append-only: DELETE refused');
END;
