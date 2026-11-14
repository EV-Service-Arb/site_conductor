-- Every costly action (sa_core::prices::Item), as it was done or told: what vendors bill.
CREATE TABLE usage (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	at INTEGER NOT NULL,
	item TEXT NOT NULL,
	units INTEGER NOT NULL CHECK (units > 0),
	bill TEXT NOT NULL CHECK (bill IN ('ours', 'theirs')),
	-- who told: `conductor`, a queries cache scheme, or a feed's service
	source TEXT NOT NULL,
	user_id BLOB REFERENCES users (user_id),
	key TEXT
) STRICT;

CREATE INDEX usage_at ON usage (at);
CREATE INDEX usage_item_at ON usage (item, at);

CREATE TRIGGER usage_no_update BEFORE UPDATE ON usage
BEGIN
	SELECT RAISE(ABORT, 'usage is append-only: UPDATE refused');
END;

CREATE TRIGGER usage_no_delete BEFORE DELETE ON usage
BEGIN
	SELECT RAISE(ABORT, 'usage is append-only: DELETE refused');
END;

-- Where each service's usage feed is read up to.
CREATE TABLE usage_cursors (
	source TEXT PRIMARY KEY,
	after INTEGER NOT NULL
) STRICT;

-- What one unit of an item costs us, in millionths of a dollar: the admin's.
CREATE TABLE unit_costs (
	item TEXT PRIMARY KEY,
	usd_micros INTEGER NOT NULL CHECK (usd_micros >= 0),
	set_by TEXT NOT NULL,
	set_at INTEGER NOT NULL
) STRICT;

INSERT INTO usage (at, item, units, bill, source, user_id, key)
SELECT at, 'street_view.panorama', 1, 'ours', 'conductor', user_id, printf('%.6f,%.6f', lat, lng) FROM street_views ORDER BY id;
DROP TABLE street_views;
