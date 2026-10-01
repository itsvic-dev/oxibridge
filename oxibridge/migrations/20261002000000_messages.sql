CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    group_name TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE message_links (
    message_id INTEGER NOT NULL REFERENCES messages (id) ON DELETE CASCADE,
    backend TEXT NOT NULL,
    chat TEXT NOT NULL,
    platform_id TEXT NOT NULL,
    PRIMARY KEY (backend, chat, platform_id)
);

CREATE INDEX message_links_by_message ON message_links (message_id, backend);
