DROP TRIGGER gmbs_purged_once;
DROP TRIGGER gmbs_removed_stays;
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
DROP TRIGGER gmails_only_removed;
CREATE TRIGGER gmails_only_removed BEFORE UPDATE ON gmails
WHEN NEW.id IS NOT OLD.id OR NEW.owner IS NOT OLD.owner OR NEW.gmail IS NOT OLD.gmail OR NEW.created_at IS NOT OLD.created_at
	OR OLD.removed_at IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'a gmail is only removed, once: UPDATE refused');
END;
DROP TABLE trash_settings;
ALTER TABLE gmbs DROP COLUMN purged_at;
ALTER TABLE gmails DROP COLUMN purged_at;
