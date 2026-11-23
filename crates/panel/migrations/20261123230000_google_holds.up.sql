-- What an admin set each Google account to (sa_core::google::Hold): the latest is held, every one kept.
CREATE TABLE google_holds (
	account TEXT NOT NULL,
	hold TEXT NOT NULL CHECK (json_valid(hold)),
	at INTEGER NOT NULL,
	by TEXT NOT NULL
) STRICT;

CREATE INDEX google_holds_latest ON google_holds (account, at);

CREATE TRIGGER google_holds_no_delete BEFORE DELETE ON google_holds
BEGIN
	SELECT RAISE(ABORT, 'a hold is kept: DELETE refused');
END;

CREATE TRIGGER google_holds_no_update BEFORE UPDATE ON google_holds
BEGIN
	SELECT RAISE(ABORT, 'a hold is kept: UPDATE refused');
END;
