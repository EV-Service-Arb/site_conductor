-- NULL: each removal counts from its removed_at; else from the later of the two (never_purge turned off then).
ALTER TABLE trash_settings ADD COLUMN counted_from INTEGER;
