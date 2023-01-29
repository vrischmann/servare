ALTER TABLE feed_entries DROP COLUMN creator;
ALTER TABLE feed_entries ADD COLUMN authors text[] NULL;
