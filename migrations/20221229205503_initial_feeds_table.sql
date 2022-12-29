CREATE TABLE feeds(
  id uuid NOT NULL,
  user_id uuid NOT NULL,

  url text NOT NULL,
  title text,
  site_link text,
  description text,

  created_at timestamptz NOT NULL,
  last_checked_at timestamptz,

  PRIMARY KEY (id),
  FOREIGN KEY (user_id) REFERENCES users(id)
);

CREATE TABLE feed_items(
  id uuid NOT NULL,
  feed_id uuid NOT NULL,

  title text NOT NULL,
  url text NOT NULL,
  created_at timestamptz NOT NULL,
  creator text NOT NULL,
  description text NOT NULL,

  PRIMARY KEY (feed_id, id),
  FOREIGN KEY (feed_id) REFERENCES feeds(id)
);
