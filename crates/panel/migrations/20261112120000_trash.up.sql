-- A removed gmail or GBP waits in its owner's trash: restored (removed_at cleared) until it is
-- purged (purged_at, once), after its owner's kept days (docs/ARCHITECTURE.md, Tokens and GBPs).
ALTER TABLE gmails ADD COLUMN purged_at INTEGER CHECK (purged_at IS NULL OR removed_at IS NOT NULL);
ALTER TABLE gmbs ADD COLUMN purged_at INTEGER CHECK (purged_at IS NULL OR removed_at IS NOT NULL);

-- No row: 30 days, and never_purge as the deployment says (dev sign-in keeps everything).
CREATE TABLE trash_settings (
	user_id BLOB PRIMARY KEY REFERENCES users (user_id),
	keep_days INTEGER NOT NULL CHECK (keep_days BETWEEN 1 AND 90),
	-- NULL: as the deployment says.
	never_purge INTEGER CHECK (never_purge IN (0, 1))
) STRICT;

DROP TRIGGER gmails_only_removed;
CREATE TRIGGER gmails_only_removed BEFORE UPDATE ON gmails
WHEN NEW.id IS NOT OLD.id OR NEW.owner IS NOT OLD.owner OR NEW.gmail IS NOT OLD.gmail OR NEW.created_at IS NOT OLD.created_at
	OR (OLD.removed_at IS NULL AND NEW.purged_at IS NOT NULL)
	OR (OLD.removed_at IS NOT NULL AND NOT (NEW.role IS OLD.role AND OLD.purged_at IS NULL AND (
		(NEW.removed_at IS NULL AND NEW.purged_at IS NULL)
		OR (NEW.removed_at IS OLD.removed_at AND NEW.purged_at IS NOT NULL))))
BEGIN
	SELECT RAISE(ABORT, 'a gmail is removed, restored until purged, purged once: UPDATE refused');
END;

DROP TRIGGER gmbs_removed_stays;
CREATE TRIGGER gmbs_removed_stays BEFORE UPDATE ON gmbs
WHEN OLD.removed_at IS NOT NULL AND NOT (
	NEW.id IS OLD.id AND NEW.owner IS OLD.owner AND NEW.gmail_id IS OLD.gmail_id AND NEW.place_id IS OLD.place_id
	AND NEW.name IS OLD.name AND NEW.address IS OLD.address AND NEW.state IS OLD.state AND NEW.country IS OLD.country
	AND NEW.country_by IS OLD.country_by AND NEW.phone IS OLD.phone AND NEW.lat IS OLD.lat AND NEW.lng IS OLD.lng
	AND NEW.placed_by IS OLD.placed_by AND NEW.created_at IS OLD.created_at
	AND NEW.areas IS OLD.areas AND NEW.hidden_address IS OLD.hidden_address AND NEW.customers_visit IS OLD.customers_visit
	AND NEW.film IS OLD.film AND (
		-- its trade given up, so the trade can be deleted: no service stores a trade id
		(NEW.trade_id IS NULL AND NEW.removed_at IS OLD.removed_at AND NEW.purged_at IS OLD.purged_at)
		OR (NEW.trade_id IS OLD.trade_id AND OLD.purged_at IS NULL AND (
			(NEW.removed_at IS NULL AND NEW.purged_at IS NULL)
			OR (NEW.removed_at IS OLD.removed_at AND NEW.purged_at IS NOT NULL)))))
BEGIN
	SELECT RAISE(ABORT, 'a removed GMB is restored until purged, purged once, or gives its trade up: UPDATE refused');
END;

CREATE TRIGGER gmbs_purged_once BEFORE UPDATE OF purged_at ON gmbs
WHEN OLD.removed_at IS NULL AND NEW.purged_at IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'a live GMB is not purged: UPDATE refused');
END;
