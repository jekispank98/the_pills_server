-- Databases set up by hand before migrations ran automatically lack the
-- indexes from 202610011200_add_medications_and_intakes.sql. No-op elsewhere.
CREATE INDEX IF NOT EXISTS medications_user_account_id_idx ON medications (user_account_id);
CREATE INDEX IF NOT EXISTS medication_person_cross_ref_person_id_idx ON medication_person_cross_ref (person_id);
CREATE INDEX IF NOT EXISTS intakes_user_account_id_idx ON intakes (user_account_id);
CREATE INDEX IF NOT EXISTS intakes_medication_id_idx ON intakes (medication_id);
CREATE INDEX IF NOT EXISTS intake_person_cross_ref_person_id_idx ON intake_person_cross_ref (person_id);
