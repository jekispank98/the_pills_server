CREATE TABLE users
(
    id               SERIAL PRIMARY KEY,
    login            TEXT NOT NULL UNIQUE,
    password         TEXT NOT NULL,
    name             TEXT NOT NULL,
    token_expires_at TIMESTAMP,
    subscribed       BOOLEAN   DEFAULT false,
    created_at       TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);