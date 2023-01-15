ALTER TABLE jobs ADD COLUMN key bytea NOT NULL UNIQUE;
ALTER TABLE jobs DROP COLUMN processed_at;

CREATE TYPE job_status AS ENUM ('pending', 'running', 'done', 'failed');
ALTER TABLE jobs ADD COLUMN status job_status NOT NULL DEFAULT 'pending';
CREATE INDEX jobs_status_idx ON jobs(status);
