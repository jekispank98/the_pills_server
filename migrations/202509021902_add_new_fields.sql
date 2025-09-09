ALTER TABLE users
ADD COLUMN verified BOOLEAN DEFAULT false,
ADD COLUMN updated_at TIMESTAMP,
ADD COLUMN verification_token TEXT;
