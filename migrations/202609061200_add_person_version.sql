-- Оптимистичная конкуренция для persons: клиент шлёт последнюю известную
-- версию как base_version в UpsertPersonRequest, апдейт применяется только
-- если она совпадает с текущей (см. PersonRepositoryTrait::upsert_person).
ALTER TABLE persons ADD COLUMN version INTEGER NOT NULL DEFAULT 0;
