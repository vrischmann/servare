ALTER TABLE feed_items RENAME TO feed_entries;
ALTER TABLE feed_entries ALTER COLUMN url DROP NOT NULL;
ALTER TABLE feed_entries RENAME COLUMN description TO summary;
