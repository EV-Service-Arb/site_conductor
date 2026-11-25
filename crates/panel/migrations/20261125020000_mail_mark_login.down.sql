ALTER TABLE mail_drafts DROP COLUMN login;

UPDATE mail_cases SET mark_mail = NULL, mark_by = NULL, mark_open = 0 WHERE mark_open = 1;
ALTER TABLE mail_cases DROP COLUMN mark_open;
ALTER TABLE mail_cases RENAME COLUMN mark_by TO done_by;
ALTER TABLE mail_cases RENAME COLUMN mark_mail TO done_mail;
