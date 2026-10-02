ALTER TABLE messages ADD COLUMN backend TEXT;
ALTER TABLE messages ADD COLUMN content TEXT;
ALTER TABLE messages ADD COLUMN in_reply_to INTEGER;

CREATE TABLE reactions (
    message_id INTEGER NOT NULL REFERENCES messages (id) ON DELETE CASCADE,
    backend TEXT NOT NULL,
    emoji TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (message_id, backend, emoji)
);
