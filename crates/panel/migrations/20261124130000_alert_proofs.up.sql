-- The links mailed to addresses to prove them, for a day: what bounds how many.
CREATE TABLE alert_proofs (
	user_id BLOB NOT NULL CHECK (length(user_id) = 16),
	address_tag BLOB NOT NULL CHECK (length(address_tag) = 32),
	at INTEGER NOT NULL
) STRICT;
