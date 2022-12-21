CREATE TABLE users(
  id uuid NOT NULL,
  name text NOT NULL,
  created_at timestamptz NOT NULL,
  confirmed boolean NOT NULL,
  confirmed_at timestamptz NOT NULL,
  PRIMARY KEY (id)
);

CREATE INDEX users_by_confirmed ON users(confirmed);
