-- A hold now carries its account's credits: each account's latest gets none, until an admin sets them.
INSERT INTO google_holds (account, hold, at, by)
SELECT account, json_set(hold, '$.credits', json('[]')), at, 'migration: credits into holds'
FROM google_holds h
WHERE rowid = (SELECT rowid FROM google_holds o WHERE o.account = h.account ORDER BY at DESC, rowid DESC LIMIT 1);
