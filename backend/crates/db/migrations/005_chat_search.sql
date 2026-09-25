CREATE VIRTUAL TABLE IF NOT EXISTS chats_fts USING fts5(
    chat_id UNINDEXED,
    user_id UNINDEXED,
    title,
    body,
    tokenize = 'trigram'
);

CREATE TABLE IF NOT EXISTS usage_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    model TEXT NOT NULL,
    day TEXT NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL,
    cost REAL NOT NULL DEFAULT 0
);
