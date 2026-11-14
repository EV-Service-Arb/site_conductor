-- A draft's candidate spots, in the order they are weighed (docs/ARCHITECTURE.md, Tokens and GBPs).
-- The first is the draft's point: gmbs.lat/lng, placed by hand, written with them.
CREATE TABLE gmb_spots (
	gmb_id INTEGER NOT NULL REFERENCES gmbs (id),
	pos INTEGER NOT NULL CHECK (pos >= 0),
	lat REAL NOT NULL CHECK (lat BETWEEN -90 AND 90),
	lng REAL NOT NULL CHECK (lng BETWEEN -180 AND 180),
	-- what it was picked by: an address, a business nearby
	label TEXT CHECK (label = trim(label) AND label != ''),
	note TEXT CHECK (note = trim(note) AND note != ''),
	PRIMARY KEY (gmb_id, pos)
) STRICT;

INSERT INTO gmb_spots (gmb_id, pos, lat, lng, label)
SELECT id, 0, lat, lng, address FROM gmbs WHERE state = 'draft' AND lat IS NOT NULL;
