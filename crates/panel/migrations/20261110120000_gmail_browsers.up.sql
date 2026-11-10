-- A gmail's antidetect browser (docs/ARCHITECTURE.md, Browsers): one per gmail, its proxy sealed
-- under PANEL_DATA_KEY with the gmail id as associated data, and no proxy two gmails'. A GBP's
-- hand-typed profile goes: the browser is its gmail's.

CREATE TABLE gmail_browsers (
	gmail_id INTEGER PRIMARY KEY REFERENCES gmails (id) ON DELETE CASCADE,
	provider TEXT NOT NULL CHECK (provider IN ('gologin')),
	device TEXT NOT NULL CHECK (device != ''),
	proxy_sealed BLOB NOT NULL,
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	-- HMAC-SHA256 of the normalized proxy URL under a key derived from PANEL_DATA_KEY.
	proxy_tag BLOB NOT NULL UNIQUE CHECK (length(proxy_tag) = 32),
	-- What a person tells the proxy by: host:port.
	endpoint TEXT NOT NULL CHECK (endpoint != ''),
	-- The provider's profile, made at the first start; its noise read back from it then.
	provider_profile TEXT CHECK (provider_profile != ''),
	noise_canvas REAL,
	noise_webgl REAL,
	noise_rects REAL,
	noise_audio REAL,
	exit_ip TEXT NOT NULL,
	exit_country TEXT NOT NULL CHECK (length(exit_country) = 2),
	exit_city TEXT NOT NULL,
	exit_zone TEXT NOT NULL,
	exit_mobile INTEGER NOT NULL CHECK (exit_mobile IN (0, 1)),
	set_at INTEGER NOT NULL,
	checked_at INTEGER NOT NULL,
	-- NULL: the last check went through the proxy, out in the gmail's GBPs' country.
	check_error TEXT,
	CHECK ((provider_profile IS NULL) = (noise_canvas IS NULL) AND (noise_canvas IS NULL) = (noise_webgl IS NULL)
		AND (noise_webgl IS NULL) = (noise_rects IS NULL) AND (noise_rects IS NULL) = (noise_audio IS NULL))
) STRICT;

DROP TRIGGER gmbs_removed_stays;
ALTER TABLE gmbs DROP COLUMN profile;
CREATE TRIGGER gmbs_removed_stays BEFORE UPDATE ON gmbs
WHEN OLD.removed_at IS NOT NULL AND NOT (NEW.trade_id IS NULL
	AND NEW.id IS OLD.id AND NEW.owner IS OLD.owner AND NEW.gmail_id IS OLD.gmail_id AND NEW.place_id IS OLD.place_id
	AND NEW.name IS OLD.name AND NEW.address IS OLD.address AND NEW.state IS OLD.state AND NEW.country IS OLD.country
	AND NEW.country_by IS OLD.country_by AND NEW.phone IS OLD.phone AND NEW.lat IS OLD.lat AND NEW.lng IS OLD.lng
	AND NEW.placed_by IS OLD.placed_by AND NEW.created_at IS OLD.created_at AND NEW.removed_at IS OLD.removed_at
	AND NEW.areas IS OLD.areas AND NEW.hidden_address IS OLD.hidden_address AND NEW.customers_visit IS OLD.customers_visit
	AND NEW.film IS OLD.film)
BEGIN
	SELECT RAISE(ABORT, 'a removed GMB changes no more but to give its trade up: UPDATE refused');
END;
