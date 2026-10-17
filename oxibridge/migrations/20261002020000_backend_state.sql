CREATE TABLE backend_state (
    backend TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (backend, key)
);
