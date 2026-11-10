-- Multilogin beside GoLogin: its cloud phone (`multilogin`) and its browser (`multilogin_browser`).
-- `device` is a phone's model, drawn by the conductor; `provider_profile` is made at the set-up for
-- Multilogin's, at the first start for GoLogin's, whose noise alone is kept. `film_material`: a
-- phone's film in Multilogin's Library.

CREATE TABLE gmail_browsers_new (
	gmail_id INTEGER PRIMARY KEY REFERENCES gmails (id) ON DELETE CASCADE,
	provider TEXT NOT NULL CHECK (provider IN ('multilogin', 'multilogin_browser', 'gologin')),
	device TEXT NOT NULL CHECK (device != ''),
	proxy_sealed BLOB NOT NULL,
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	-- HMAC-SHA256 of the normalized proxy URL under a key derived from PANEL_DATA_KEY.
	proxy_tag BLOB NOT NULL UNIQUE CHECK (length(proxy_tag) = 32),
	-- What a person tells the proxy by: host:port.
	endpoint TEXT NOT NULL CHECK (endpoint != ''),
	provider_profile TEXT CHECK (provider_profile != ''),
	noise_canvas REAL,
	noise_webgl REAL,
	noise_rects REAL,
	noise_audio REAL,
	film_material TEXT CHECK (film_material != ''),
	exit_ip TEXT NOT NULL,
	exit_country TEXT NOT NULL CHECK (length(exit_country) = 2),
	exit_city TEXT NOT NULL,
	exit_zone TEXT NOT NULL,
	exit_mobile INTEGER NOT NULL CHECK (exit_mobile IN (0, 1)),
	set_at INTEGER NOT NULL,
	checked_at INTEGER NOT NULL,
	-- NULL: the last check went through the proxy, out in the gmail's GBPs' country.
	check_error TEXT,
	CHECK ((noise_canvas IS NULL) = (noise_webgl IS NULL) AND (noise_webgl IS NULL) = (noise_rects IS NULL)
		AND (noise_rects IS NULL) = (noise_audio IS NULL)),
	CHECK ((noise_canvas IS NOT NULL) = (provider = 'gologin' AND provider_profile IS NOT NULL)),
	CHECK (provider = 'gologin' OR provider_profile IS NOT NULL),
	CHECK (film_material IS NULL OR provider = 'multilogin')
) STRICT;

INSERT INTO gmail_browsers_new (gmail_id, provider, device, proxy_sealed, data_key_fp, proxy_tag, endpoint, provider_profile,
	noise_canvas, noise_webgl, noise_rects, noise_audio, exit_ip, exit_country, exit_city, exit_zone, exit_mobile, set_at,
	checked_at, check_error)
SELECT gmail_id, provider, device, proxy_sealed, data_key_fp, proxy_tag, endpoint, provider_profile,
	noise_canvas, noise_webgl, noise_rects, noise_audio, exit_ip, exit_country, exit_city, exit_zone, exit_mobile, set_at,
	checked_at, check_error
FROM gmail_browsers;
DROP TABLE gmail_browsers;
ALTER TABLE gmail_browsers_new RENAME TO gmail_browsers;
