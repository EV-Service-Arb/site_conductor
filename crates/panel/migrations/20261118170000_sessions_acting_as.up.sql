-- Whose account the session acts in, lending its own user's permissions; NULL: its own.
ALTER TABLE sessions ADD COLUMN acting_as BLOB REFERENCES users (user_id);
