-- Background jobs stuff

CREATE TYPE job_status AS ENUM (
    'pending',
    'failed'
);

CREATE TABLE jobs (
    id uuid NOT NULL,
    data jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    error text,
    key bytea NOT NULL,
    status public.job_status DEFAULT 'pending'::public.job_status NOT NULL
);
ALTER TABLE ONLY jobs ADD CONSTRAINT jobs_key_key UNIQUE (key);
ALTER TABLE ONLY jobs ADD CONSTRAINT jobs_pkey PRIMARY KEY (id);


-- Users and authentication

CREATE TABLE users (
    id uuid NOT NULL,
    name text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    confirmed boolean DEFAULT false NOT NULL,
    confirmed_at timestamp with time zone,
    email text NOT NULL,
    password_hash text NOT NULL
);
ALTER TABLE ONLY users ADD CONSTRAINT users_pkey PRIMARY KEY (id);
CREATE INDEX users_by_confirmed ON users USING btree (confirmed);

CREATE TABLE sessions (
    id uuid NOT NULL,
    state jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL
);
ALTER TABLE ONLY sessions ADD CONSTRAINT sessions_pkey PRIMARY KEY (id);


-- Feeds and stuff


CREATE TABLE feeds (
    id uuid NOT NULL,
    user_id uuid NOT NULL,
    url text NOT NULL,
    title text NOT NULL,
    site_link text NOT NULL,
    description text NOT NULL,
    added_at timestamp with time zone NOT NULL,
    site_favicon bytea,
    has_favicon boolean
);
ALTER TABLE feeds ADD CONSTRAINT feeds_pkey PRIMARY KEY (id);
ALTER TABLE feeds ADD CONSTRAINT feeds_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id);


CREATE TABLE feed_entries (
    id uuid NOT NULL,
    feed_id uuid NOT NULL,
    title text NOT NULL,
    url text,
    created_at timestamp with time zone NOT NULL,
    summary text NOT NULL,
    authors text[]
);
ALTER TABLE feed_entries ADD CONSTRAINT feed_items_pkey PRIMARY KEY (feed_id, id);
ALTER TABLE feed_entries ADD CONSTRAINT feed_items_feed_id_fkey FOREIGN KEY (feed_id) REFERENCES feeds(id);
