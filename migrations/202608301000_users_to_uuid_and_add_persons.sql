-- ID-стратегия (см. pills_models_and_methods.md): полный переход на UUID.
-- users.id меняется на UUID, генерируется сервером — регистрация и так требует
-- сети, офлайн-генерация тут не нужна. persons.id, наоборот, генерируется на
-- клиенте (offline-first): новую персону можно создать без сети.
--
-- До этой миграции persons не было ни одной ссылающейся таблицы на users.id,
-- поэтому смена типа PK ничего не каскадирует.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

ALTER TABLE users
    ADD COLUMN id_uuid UUID NOT NULL DEFAULT gen_random_uuid();

-- Миграционная история этой таблицы несколько раз пересоздавала её через
-- `CREATE TABLE users_new ... RENAME TO users` (см. более старые файлы в этой
-- папке), а `RENAME TABLE` не переименовывает constraints — так что имя PK
-- constraint'а сейчас не факт, что `users_pkey`. Находим его динамически,
-- вместо того чтобы гадать (`users_pkey` / `users_new_pkey` / ...).
DO $$
DECLARE
    pk_name text;
BEGIN
    SELECT conname INTO pk_name
    FROM pg_constraint
    WHERE conrelid = 'users'::regclass AND contype = 'p';

    IF pk_name IS NOT NULL THEN
        EXECUTE format('ALTER TABLE users DROP CONSTRAINT %I', pk_name);
    END IF;
END $$;

ALTER TABLE users
    DROP COLUMN id;

ALTER TABLE users
    RENAME COLUMN id_uuid TO id;

ALTER TABLE users
    ADD PRIMARY KEY (id);

ALTER TABLE users
    ALTER COLUMN id SET DEFAULT gen_random_uuid();

-- Профильные поля из Android `domain.models.User`, которых в БД ещё не было.
-- Подписка (isPremium/subscriptionType/subscriptionEndDate) сюда сознательно
-- не входит — это отдельная задача про биллинг, `subscribed` уже есть.
ALTER TABLE users
    ADD COLUMN photo_url TEXT,
    ADD COLUMN time_zone TEXT,
    ADD COLUMN locale TEXT;

CREATE TABLE persons (
    id               UUID PRIMARY KEY, -- задаётся клиентом при создании
    user_account_id  UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    relation         TEXT NOT NULL DEFAULT 'OTHER'
        CHECK (relation IN ('SELF', 'CHILD', 'PARENT', 'SPOUSE', 'OTHER')),
    is_user          BOOLEAN NOT NULL DEFAULT false,
    birth_date       TIMESTAMP,
    gender           TEXT NOT NULL DEFAULT 'UNSPECIFIED'
        CHECK (gender IN ('MALE', 'FEMALE', 'UNSPECIFIED')),
    weight_kg        DOUBLE PRECISION,
    color_tag        BIGINT,
    notes            TEXT,
    is_active        BOOLEAN NOT NULL DEFAULT true,
    photo_url        TEXT,
    created_at       TIMESTAMP NOT NULL DEFAULT now(),
    updated_at       TIMESTAMP NOT NULL DEFAULT now()
);

CREATE INDEX persons_user_account_id_idx ON persons (user_account_id);

-- Один SELF-профиль на аккаунт — соответствует Android UserRepositoryImpl.signUp,
-- который создаёт ровно один Person(relation=SELF, isUser=true) при регистрации.
CREATE UNIQUE INDEX persons_one_self_per_account_idx
    ON persons (user_account_id)
    WHERE is_user;
