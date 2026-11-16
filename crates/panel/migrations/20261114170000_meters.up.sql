-- A vendor's own count of a meter (sa_core::meters) this period, as of `through`: one row a sync,
-- only ever added.
CREATE TABLE meter_readings (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	meter TEXT NOT NULL,
	period_start INTEGER NOT NULL,
	units INTEGER NOT NULL CHECK (units >= 0),
	-- gross, before credits, in millionths of `currency`
	cost_micros INTEGER NOT NULL,
	currency TEXT NOT NULL,
	through INTEGER NOT NULL,
	synced_at INTEGER NOT NULL
) STRICT;

CREATE INDEX meter_readings_meter ON meter_readings (meter, id);
CREATE INDEX meter_readings_synced_at ON meter_readings (synced_at);

CREATE TRIGGER meter_readings_no_update BEFORE UPDATE ON meter_readings
BEGIN
	SELECT RAISE(ABORT, 'meter_readings is append-only: UPDATE refused');
END;

CREATE TRIGGER meter_readings_no_delete BEFORE DELETE ON meter_readings
BEGIN
	SELECT RAISE(ABORT, 'meter_readings is append-only: DELETE refused');
END;
