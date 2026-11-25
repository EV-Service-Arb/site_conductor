-- A person's mark on a case, done or open, holds until Google speaks in it.
ALTER TABLE mail_cases RENAME COLUMN done_mail TO mark_mail;
ALTER TABLE mail_cases RENAME COLUMN done_by TO mark_by;
ALTER TABLE mail_cases ADD COLUMN mark_open INTEGER NOT NULL DEFAULT 0 CHECK (mark_open = 0 OR (mark_open = 1 AND mark_mail IS NOT NULL));

-- Gmail refused the login it was sent with
ALTER TABLE mail_drafts ADD COLUMN login INTEGER NOT NULL DEFAULT 0 CHECK (login = 0 OR (login = 1 AND error IS NOT NULL));
UPDATE mail_drafts SET login = 1 WHERE state = 'failed' AND (error LIKE '%(534)%' OR error LIKE '%(535)%');
