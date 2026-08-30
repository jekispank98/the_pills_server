-- Разделяем «срок жизни токена подтверждения email» и сессионную аутентификацию.
-- До этой миграции колонка token_expires_at использовалась под обе задачи:
-- register писал в неё срок жизни письма, а login перезатирал её при каждом входе,
-- ломая состояние верификации. Сессия теперь живёт исключительно в JWT и в БД не хранится.
ALTER TABLE users
    RENAME COLUMN token_expires_at TO verification_token_expires_at;

-- Пользователи, пришедшие через Google, пароля не имеют.
ALTER TABLE users
    ALTER COLUMN password DROP NOT NULL;

-- Стабильный идентификатор аккаунта Google (claim `sub`).
-- Привязываемся к нему, а не к email: email в Google-аккаунте может меняться.
ALTER TABLE users
    ADD COLUMN google_sub TEXT;

ALTER TABLE users
    ADD CONSTRAINT users_google_sub_key UNIQUE (google_sub);

-- Поиск по токену подтверждения идёт на каждый клик из письма.
CREATE INDEX IF NOT EXISTS users_verification_token_idx
    ON users (verification_token)
    WHERE verification_token IS NOT NULL;

-- У аккаунта должен быть хотя бы один способ входа.
ALTER TABLE users
    ADD CONSTRAINT users_has_credentials
        CHECK (password IS NOT NULL OR google_sub IS NOT NULL);
