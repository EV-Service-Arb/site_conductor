-- A gmail's connections (docs/ARCHITECTURE.md, Tokens and GBPs): its inbox over IMAP and its own
-- Maps key, each sealed under PANEL_DATA_KEY with the gmail id as associated data, and deleted
-- with it: no service stores an id of them. And what a Places request on the shared keys costs.

CREATE TABLE gmail_imap (
	gmail_id INTEGER PRIMARY KEY REFERENCES gmails (id),
	host TEXT NOT NULL CHECK (host = lower(trim(host)) AND host != ''),
	username TEXT NOT NULL CHECK (username != ''),
	password_sealed BLOB NOT NULL,
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	set_at INTEGER NOT NULL,
	checked_at INTEGER NOT NULL,
	-- NULL: the last check logged in.
	check_error TEXT
) STRICT;

CREATE TABLE gmail_maps_keys (
	gmail_id INTEGER PRIMARY KEY REFERENCES gmails (id),
	key_sealed BLOB NOT NULL,
	data_key_fp BLOB NOT NULL CHECK (length(data_key_fp) = 32),
	-- What a person tells the key by.
	last4 TEXT NOT NULL CHECK (length(last4) = 4),
	set_at INTEGER NOT NULL,
	checked_at INTEGER NOT NULL,
	-- NULL: Google answered the last check.
	check_error TEXT
) STRICT;

-- Tokens per request on the shared keys, by sa_places' SKU; a SKU without a row is not sold.
CREATE TABLE places_prices (
	sku TEXT PRIMARY KEY CHECK (sku IN ('details', 'search')),
	tokens INTEGER NOT NULL CHECK (tokens >= 0),
	set_by TEXT NOT NULL,
	set_at INTEGER NOT NULL
) STRICT;
