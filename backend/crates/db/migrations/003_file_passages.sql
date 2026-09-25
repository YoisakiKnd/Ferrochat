CREATE VIRTUAL TABLE IF NOT EXISTS file_passages USING fts5(
    file_id UNINDEXED,
    filename UNINDEXED,
    page UNINDEXED,
    body,
    tokenize = 'trigram'
);
