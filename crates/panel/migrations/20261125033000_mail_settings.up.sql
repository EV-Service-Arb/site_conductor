-- How a person's mail with Google is pursued. No row: `sa_core::mail::Settings::default`; its bounds are `LEAST` and `MOST`.
CREATE TABLE mail_settings (
	user_id BLOB PRIMARY KEY REFERENCES users (user_id),
	hold_minutes INTEGER NOT NULL CHECK (hold_minutes BETWEEN 15 AND 720),
	apart_hours INTEGER NOT NULL CHECK (apart_hours BETWEEN 12 AND 24),
	"limit" INTEGER NOT NULL CHECK ("limit" BETWEEN 1 AND 10),
	re_ask_hours INTEGER NOT NULL CHECK (re_ask_hours BETWEEN 24 AND 336),
	in_review_days INTEGER NOT NULL CHECK (in_review_days BETWEEN 3 AND 14),
	remind_hours INTEGER NOT NULL CHECK (remind_hours BETWEEN 12 AND 120),
	deadline_hours INTEGER NOT NULL CHECK (deadline_hours BETWEEN 12 AND 48)
) STRICT;
