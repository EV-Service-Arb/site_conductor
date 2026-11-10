DELETE FROM gmail_browsers WHERE provider != 'gologin';
CREATE TABLE gmail_browsers_old (
	gmail_id INTEGER PRIMARY KEY REFERENCES gmails (id) ON DELETE CASCADE,
	provider TEXT NOT NULL CHECK (provider IN ('gologin')),
	device TEXT NOT NULL CHECK (device != ''),
	proxy_sealed BLOB NOT NULL,
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	proxy_tag BLOB NOT NULL UNIQUE CHECK (length(proxy_tag) = 32),
	endpoint TEXT NOT NULL CHECK (endpoint != ''),
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
	check_error TEXT,
	CHECK ((provider_profile IS NULL) = (noise_canvas IS NULL) AND (noise_canvas IS NULL) = (noise_webgl IS NULL)
		AND (noise_webgl IS NULL) = (noise_rects IS NULL) AND (noise_rects IS NULL) = (noise_audio IS NULL))
) STRICT;
INSERT INTO gmail_browsers_old SELECT gmail_id, provider, device, proxy_sealed, data_key_fp, proxy_tag, endpoint, provider_profile,
	noise_canvas, noise_webgl, noise_rects, noise_audio, exit_ip, exit_country, exit_city, exit_zone, exit_mobile, set_at,
	checked_at, check_error FROM gmail_browsers;
DROP TABLE gmail_browsers;
ALTER TABLE gmail_browsers_old RENAME TO gmail_browsers;
