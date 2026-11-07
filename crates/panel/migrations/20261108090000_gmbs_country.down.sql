DROP TRIGGER gmbs_only_removed;
ALTER TABLE gmbs DROP COLUMN country_by;
ALTER TABLE gmbs DROP COLUMN country;

CREATE TRIGGER gmbs_only_removed BEFORE UPDATE ON gmbs
WHEN NEW.id IS NOT OLD.id OR NEW.gmail_id IS NOT OLD.gmail_id OR NEW.place_id IS NOT OLD.place_id OR NEW.name IS NOT OLD.name
	OR NEW.address IS NOT OLD.address OR NEW.gbp_account IS NOT OLD.gbp_account OR NEW.gbp_location IS NOT OLD.gbp_location
	OR NEW.created_at IS NOT OLD.created_at OR OLD.removed_at IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'a GMB is only removed, once: UPDATE refused');
END;
