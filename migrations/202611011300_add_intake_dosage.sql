ALTER TABLE intakes ADD COLUMN IF NOT EXISTS dosage DOUBLE PRECISION;

UPDATE intakes
SET dosage = medications.dosage
FROM medications
WHERE intakes.medication_id = medications.id AND intakes.dosage IS NULL;

ALTER TABLE intakes ALTER COLUMN dosage SET NOT NULL;
