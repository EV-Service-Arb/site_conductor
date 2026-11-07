-- The assistant's chats: the conductor's own; no service stores their ids, so they are deleted.
CREATE TABLE assistant_chats (
	id BLOB PRIMARY KEY CHECK (length(id) = 16),
	owner BLOB NOT NULL REFERENCES users (user_id),
	title TEXT CHECK (title IS NULL OR (title = trim(title) AND title != '')),
	general TEXT NOT NULL CHECK (general IN ('none', 'view')),
	-- The turns as the server checked them.
	turns TEXT NOT NULL CHECK (json_valid(turns) AND json_type(turns) = 'array'),
	version INTEGER NOT NULL,
	created_at INTEGER NOT NULL,
	updated_at INTEGER NOT NULL
) STRICT;

CREATE INDEX assistant_chats_owner ON assistant_chats (owner);

CREATE TABLE assistant_members (
	chat BLOB NOT NULL REFERENCES assistant_chats (id) ON DELETE CASCADE,
	user_id BLOB NOT NULL REFERENCES users (user_id),
	added_by BLOB NOT NULL REFERENCES users (user_id),
	added_at INTEGER NOT NULL,
	PRIMARY KEY (chat, user_id)
) STRICT;

CREATE INDEX assistant_members_user ON assistant_members (user_id);

-- Each person's own pin and archive of a chat.
CREATE TABLE assistant_marks (
	chat BLOB NOT NULL REFERENCES assistant_chats (id) ON DELETE CASCADE,
	user_id BLOB NOT NULL REFERENCES users (user_id),
	pinned_at INTEGER,
	archived_at INTEGER,
	PRIMARY KEY (chat, user_id)
) STRICT;
