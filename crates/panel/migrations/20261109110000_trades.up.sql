-- A GBP's category is its trade in its own country (docs/ARCHITECTURE.md, Tokens and GBPs): a
-- trade is its owner's, as a gmail is; a category the trade set up for one country. The per-GBP
-- free text `category` goes; nothing is derived from it here.

CREATE TABLE trades (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	owner BLOB NOT NULL REFERENCES users (user_id),
	name TEXT NOT NULL CHECK (name = trim(name) AND name != ''),
	created_at INTEGER NOT NULL
) STRICT;

-- Folds ASCII only: the panel compares the names folded in full before it writes.
CREATE UNIQUE INDEX trades_name ON trades (owner, lower(name));

CREATE TABLE categories (
	trade_id INTEGER NOT NULL REFERENCES trades (id) ON DELETE CASCADE,
	country TEXT NOT NULL CHECK (country GLOB '[A-Z][A-Z]'),
	-- In sa_core::gbp::keywords' form.
	keywords TEXT NOT NULL CHECK (json_valid(keywords) AND json_type(keywords) = 'array' AND json_array_length(keywords) <= 50),
	-- The label typed into Google's form, in the country's language.
	google_category TEXT CHECK (google_category = trim(google_category) AND google_category != ''),
	-- Placement's trade that studies it.
	study TEXT CHECK (study = trim(study) AND study != ''),
	PRIMARY KEY (trade_id, country)
) STRICT;

ALTER TABLE gmbs DROP COLUMN category;
ALTER TABLE gmbs ADD COLUMN trade_id INTEGER REFERENCES trades (id);

CREATE INDEX gmbs_trade ON gmbs (trade_id);

CREATE TRIGGER gmbs_trade_owner_on_insert BEFORE INSERT ON gmbs
WHEN NEW.trade_id IS NOT NULL AND NEW.owner IS NOT (SELECT owner FROM trades WHERE id = NEW.trade_id)
BEGIN
	SELECT RAISE(ABORT, 'a GMB''s trade is its owner''s: INSERT refused');
END;

CREATE TRIGGER gmbs_trade_owner_on_update BEFORE UPDATE OF trade_id ON gmbs
WHEN NEW.trade_id IS NOT NULL AND NEW.owner IS NOT (SELECT owner FROM trades WHERE id = NEW.trade_id)
BEGIN
	SELECT RAISE(ABORT, 'a GMB''s trade is its owner''s: UPDATE refused');
END;

-- A removed GBP gives its trade up, so the trade can be deleted: no service stores a trade id.
DROP TRIGGER gmbs_removed_stays;
CREATE TRIGGER gmbs_removed_stays BEFORE UPDATE ON gmbs
WHEN OLD.removed_at IS NOT NULL AND NOT (NEW.trade_id IS NULL
	AND NEW.id IS OLD.id AND NEW.owner IS OLD.owner AND NEW.gmail_id IS OLD.gmail_id AND NEW.place_id IS OLD.place_id
	AND NEW.name IS OLD.name AND NEW.address IS OLD.address AND NEW.state IS OLD.state AND NEW.country IS OLD.country
	AND NEW.country_by IS OLD.country_by AND NEW.phone IS OLD.phone AND NEW.lat IS OLD.lat AND NEW.lng IS OLD.lng
	AND NEW.placed_by IS OLD.placed_by AND NEW.created_at IS OLD.created_at AND NEW.removed_at IS OLD.removed_at
	AND NEW.areas IS OLD.areas AND NEW.hidden_address IS OLD.hidden_address AND NEW.customers_visit IS OLD.customers_visit
	AND NEW.film IS OLD.film AND NEW.profile IS OLD.profile)
BEGIN
	SELECT RAISE(ABORT, 'a removed GMB changes no more but to give its trade up: UPDATE refused');
END;
