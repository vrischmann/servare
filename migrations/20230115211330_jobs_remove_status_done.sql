ALTER TABLE jobs DROP COLUMN status;

DROP TYPE job_status;
CREATE TYPE job_status AS ENUM ('pending', 'failed');

ALTER TABLE jobs ADD COLUMN status job_status NOT NULL DEFAULT 'pending';
