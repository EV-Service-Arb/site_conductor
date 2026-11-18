CREATE TRIGGER gmbs_named_as_draft BEFORE UPDATE OF name ON gmbs
WHEN NEW.name IS NOT OLD.name AND OLD.state != 'draft'
BEGIN
	SELECT RAISE(ABORT, 'a GMB is renamed only while a draft: UPDATE refused');
END;

DROP TABLE gmb_photos;
ALTER TABLE gmbs DROP COLUMN google_primary;
ALTER TABLE gmbs DROP COLUMN hours;
ALTER TABLE gmbs DROP COLUMN website;
