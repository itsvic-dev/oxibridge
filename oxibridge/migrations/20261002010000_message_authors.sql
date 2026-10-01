ALTER TABLE messages ADD COLUMN author_username TEXT;
ALTER TABLE messages ADD COLUMN author_display_name TEXT;
ALTER TABLE messages ADD COLUMN author_source TEXT;

CREATE INDEX message_links_by_platform_id ON message_links (backend, platform_id);
