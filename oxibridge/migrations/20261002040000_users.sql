CREATE TABLE users (
    backend TEXT NOT NULL,
    username TEXT NOT NULL,
    user_id TEXT NOT NULL,
    display_name TEXT,
    PRIMARY KEY (backend, username)
);
