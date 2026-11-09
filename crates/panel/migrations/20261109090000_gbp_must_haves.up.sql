-- A draft's must-haves, what Google's form asks and what stands behind it (docs/ARCHITECTURE.md,
-- Tokens and GBPs). A phone is one live GBP's, in sa_core::gbp::phone's form: the live rows'
-- rewritten to it, and the index fails the migration on two sharing one. A gmail's role on its
-- GBPs is unknown until said.

ALTER TABLE gmbs ADD COLUMN category TEXT CHECK (category = trim(category) AND category != '');
ALTER TABLE gmbs ADD COLUMN areas TEXT CHECK (json_valid(areas) AND json_type(areas) = 'array' AND json_array_length(areas) BETWEEN 1 AND 20);
-- Where the verification film is shot; shown to customers only by a storefront (customers_visit).
ALTER TABLE gmbs ADD COLUMN hidden_address TEXT CHECK (hidden_address = trim(hidden_address) AND hidden_address != '');
ALTER TABLE gmbs ADD COLUMN customers_visit INTEGER CHECK (customers_visit IN (0, 1));
-- References recorded by hand until verif_vid and Multilogin fill them.
ALTER TABLE gmbs ADD COLUMN film TEXT CHECK (film = trim(film) AND film != '');
ALTER TABLE gmbs ADD COLUMN profile TEXT CHECK (profile = trim(profile) AND profile != '');

UPDATE gmbs SET phone = replace(replace(replace(replace(replace(phone, ' ', ''), '-', ''), '(', ''), ')', ''), '.', '')
WHERE phone IS NOT NULL AND removed_at IS NULL;

CREATE UNIQUE INDEX gmbs_phone ON gmbs (phone) WHERE removed_at IS NULL;

ALTER TABLE gmails ADD COLUMN role TEXT CHECK (role IN ('owner', 'manager'));
