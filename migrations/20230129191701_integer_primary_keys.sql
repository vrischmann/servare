ALTER TABLE feed_entries DROP CONSTRAINT feed_items_feed_id_fkey;
ALTER TABLE feed_entries DROP CONSTRAINT feed_items_pkey;
ALTER TABLE feeds DROP CONSTRAINT feeds_pkey;

ALTER TABLE feeds DROP column id;
ALTER TABLE feed_entries DROP column id;
ALTER TABLE feed_entries DROP column feed_id;

ALTER TABLE feeds ADD COLUMN id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY;
ALTER TABLE feed_entries ADD COLUMN id bigint GENERATED ALWAYS AS IDENTITY;
ALTER TABLE feed_entries ADD COLUMN feed_id bigint;
ALTER TABLE feed_entries ADD CONSTRAINT feed_entries_pkey PRIMARY KEY (id, feed_id);
ALTER TABLE feed_entries ADD CONSTRAINT feed_entries_feed_id_fkey FOREIGN KEY (feed_id) REFERENCES feeds(id);
