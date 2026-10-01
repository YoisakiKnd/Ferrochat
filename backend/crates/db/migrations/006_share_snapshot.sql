ALTER TABLE chats ADD COLUMN shared_chat_json TEXT;

-- Existing links keep their current content when upgrading.
UPDATE chats SET shared_chat_json = chat_json WHERE share_id IS NOT NULL;
