DROP TABLE oauth_codes;
DELETE FROM sessions WHERE client_id IS NOT NULL;
ALTER TABLE sessions DROP COLUMN client_id;
DROP TABLE oauth_clients;
