-- User-uploaded images. Stored as blobs directly in SQLite because the
-- expected scale is small (a few MB per document, low single-digit users
-- on the CX23 box) and it keeps backups one-file-simple. Per-image and
-- per-user quotas are enforced in the application layer.
CREATE TABLE IF NOT EXISTS images (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  filename TEXT NOT NULL,
  content_type TEXT NOT NULL,
  size_bytes INTEGER NOT NULL,
  sha256 TEXT NOT NULL,
  data BLOB NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_images_user ON images(user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_images_sha ON images(user_id, sha256);
