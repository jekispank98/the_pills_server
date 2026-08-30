-- Весь код (User, DTO, db.rs) уже давно обращается к колонке `email`, но
-- таблица со времён первой миграции называла её `login`. Из-за этого любой
-- запрос к users падал бы с "column email does not exist".
ALTER TABLE users
    RENAME COLUMN login TO email;
