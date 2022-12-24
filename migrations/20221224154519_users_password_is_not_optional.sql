ALTER TABLE users RENAME COLUMN hashed_password TO password_hash;
ALTER TABLE users ALTER COLUMN password_hash SET NOT NULL;
