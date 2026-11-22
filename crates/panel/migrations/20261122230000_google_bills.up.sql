-- Each Google account's bill from its `since` and the guard's last tick there, as its export last told
-- (sa_panel::meters::Bill): replaced whole each read, as Google revises the days it exported.
CREATE TABLE google_bills (
	account TEXT PRIMARY KEY,
	bill TEXT NOT NULL CHECK (json_valid(bill)),
	read_at INTEGER NOT NULL
) STRICT;
