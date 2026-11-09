DROP TRIGGER gmbs_removed_stays;
CREATE TRIGGER gmbs_removed_stays BEFORE UPDATE ON gmbs
WHEN OLD.removed_at IS NOT NULL
BEGIN
	SELECT RAISE(ABORT, 'a removed GMB changes no more: UPDATE refused');
END;
DROP TRIGGER gmbs_trade_owner_on_update;
DROP TRIGGER gmbs_trade_owner_on_insert;
DROP INDEX gmbs_trade;
ALTER TABLE gmbs DROP COLUMN trade_id;
ALTER TABLE gmbs ADD COLUMN category TEXT CHECK (category = trim(category) AND category != '');
DROP TABLE categories;
DROP TABLE trades;
