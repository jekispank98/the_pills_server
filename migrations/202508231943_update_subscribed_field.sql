-- migrations/0002_recreate_users_table.sql
CREATE TABLE users_new (
                           id SERIAL PRIMARY KEY,
                           login TEXT NOT NULL UNIQUE,
                           password TEXT NOT NULL,
                           name TEXT NOT NULL,
                           token_expires_at TIMESTAMP,
                           subscribed BOOLEAN DEFAULT false,
                           created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Перенесите данные если нужно
INSERT INTO users_new (id, login, password, name, token_expires_at, subscribed)
SELECT
    id,
    login,
    password,
    COALESCE(name, ''),
    token_expires_at,
    false as subscribed  -- Значение по умолчанию
FROM users;

-- Удалите старую таблицу
DROP TABLE users;

-- Переименуйте новую таблицу
ALTER TABLE users_new RENAME TO users;