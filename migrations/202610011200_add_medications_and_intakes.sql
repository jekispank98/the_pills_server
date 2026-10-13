-- Лекарства и приёмы — зеркалит Android Medication_new/Intake
-- (MedicationEntity_new/IntakeEntity). id — client-generated UUID, тот же
-- offline-first upsert-по-id паттерн, что и у persons (см.
-- 202608301000_users_to_uuid_and_add_persons.sql). user_account_id у обеих
-- таблиц — чисто серверная колонка для авторизации; в локальной Room-схеме
-- Android её нет (принадлежность там неявная, через привязанных persons).
CREATE TABLE medications (
    id               UUID PRIMARY KEY,
    user_account_id  UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    dosage           DOUBLE PRECISION NOT NULL,
    -- Фиксированный каталог форм выпуска, зеркалит Android PillsForm.id
    -- (util/Constants.kt) — lowercase-ключи, не UPPERCASE как у relation/gender.
    form             TEXT NOT NULL
        CHECK (form IN ('pellets', 'liquid', 'injection', 'powder', 'drops')),
    instruction      TEXT NOT NULL DEFAULT '',
    -- Свободная hex-строка (Android MedicationColor.hex) — без CHECK: Android
    -- сам мягко обрабатывает незнакомый hex (fallback на PEARL_GRAY), сервер
    -- не должен быть строже клиента на этот счёт.
    color_tag        TEXT NOT NULL,
    created_at       TIMESTAMP NOT NULL DEFAULT now(),
    updated_at       TIMESTAMP NOT NULL DEFAULT now(),
    version          INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX medications_user_account_id_idx ON medications (user_account_id);

-- Many-to-many, зеркалит Android MedicationPersonCrossRef. Android-сторона
-- линкует аддитивно (OnConflictStrategy.IGNORE, никогда не отвязывает сама)
-- — то же поведение здесь (см. link_persons_to_medication в db.rs).
CREATE TABLE medication_person_cross_ref (
    medication_id UUID NOT NULL REFERENCES medications (id) ON DELETE CASCADE,
    person_id     UUID NOT NULL REFERENCES persons (id) ON DELETE CASCADE,
    PRIMARY KEY (medication_id, person_id)
);

CREATE INDEX medication_person_cross_ref_person_id_idx ON medication_person_cross_ref (person_id);

CREATE TABLE intakes (
    id               UUID PRIMARY KEY,
    user_account_id  UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    medication_id    UUID NOT NULL REFERENCES medications (id) ON DELETE CASCADE,
    time             TIMESTAMP NOT NULL,
    pre_time         TIMESTAMP,
    is_completed     BOOLEAN NOT NULL DEFAULT false,
    is_passed        BOOLEAN NOT NULL DEFAULT false,
    is_canceled      BOOLEAN NOT NULL DEFAULT false,
    created_at       TIMESTAMP NOT NULL DEFAULT now(),
    updated_at       TIMESTAMP NOT NULL DEFAULT now(),
    version          INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX intakes_user_account_id_idx ON intakes (user_account_id);
CREATE INDEX intakes_medication_id_idx ON intakes (medication_id);

-- Android IntakeDao.upsertIntakeWithPersons делает delete+reinsert cross-ref'ов
-- при каждом апдейте (full replace) — в отличие от аддитивной
-- medication-линковки выше. То же самое здесь (upsert_intake_with_persons).
CREATE TABLE intake_person_cross_ref (
    intake_id UUID NOT NULL REFERENCES intakes (id) ON DELETE CASCADE,
    person_id UUID NOT NULL REFERENCES persons (id) ON DELETE CASCADE,
    PRIMARY KEY (intake_id, person_id)
);

CREATE INDEX intake_person_cross_ref_person_id_idx ON intake_person_cross_ref (person_id);
