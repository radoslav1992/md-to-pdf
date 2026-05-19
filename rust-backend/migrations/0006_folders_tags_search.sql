-- Organisation columns on documents.
--   folder: optional human-readable path string ("Work/Projects"). Stored
--           as a single TEXT for simplicity — no separate folder table.
--   tags:   JSON array of strings, e.g. '["draft","wip"]'. Validated in
--           the application layer; stored as TEXT so SQLite doesn't care.
ALTER TABLE documents ADD COLUMN folder TEXT;
ALTER TABLE documents ADD COLUMN tags TEXT;

CREATE INDEX IF NOT EXISTS idx_documents_user_folder ON documents(user_id, folder);

-- Full-text search index. Mirrors the title + content of every document,
-- kept in sync via triggers. Encrypted documents store ciphertext in
-- `content`, so only their title gets indexed (still useful for jumping
-- back to one you remember the name of).
CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
  title,
  content,
  user_id UNINDEXED,
  tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS documents_fts_ai AFTER INSERT ON documents BEGIN
  INSERT INTO documents_fts(rowid, title, content, user_id)
  VALUES (
    new.id,
    new.title,
    CASE WHEN new.is_encrypted = 0 THEN new.content ELSE '' END,
    new.user_id
  );
END;

CREATE TRIGGER IF NOT EXISTS documents_fts_au AFTER UPDATE ON documents BEGIN
  UPDATE documents_fts
  SET title = new.title,
      content = CASE WHEN new.is_encrypted = 0 THEN new.content ELSE '' END,
      user_id = new.user_id
  WHERE rowid = new.id;
END;

CREATE TRIGGER IF NOT EXISTS documents_fts_ad AFTER DELETE ON documents BEGIN
  DELETE FROM documents_fts WHERE rowid = old.id;
END;

-- Backfill the index from existing rows. Safe to run on first migration
-- because the FTS table is empty; later migrations on this index will be
-- pure deltas applied through the triggers.
INSERT INTO documents_fts(rowid, title, content, user_id)
SELECT id, title,
       CASE WHEN is_encrypted = 0 THEN content ELSE '' END,
       user_id
FROM documents;
