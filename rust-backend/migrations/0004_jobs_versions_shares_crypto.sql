-- Background jobs queue. Jobs are dequeued by a worker tokio task and the
-- result is stored back into `result_json` (or `error_message` on failure).
CREATE TABLE IF NOT EXISTS jobs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  api_key_id INTEGER,
  kind TEXT NOT NULL,                  -- "convert" | "batch"
  status TEXT NOT NULL,                -- "queued" | "running" | "done" | "failed" | "canceled"
  input_json TEXT NOT NULL,
  result_json TEXT,
  error_message TEXT,
  created_at INTEGER NOT NULL,
  started_at INTEGER,
  finished_at INTEGER,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  FOREIGN KEY (api_key_id) REFERENCES api_keys(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_jobs_user_time ON jobs(user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status, created_at);

-- Document version history. A new row is inserted before every update,
-- preserving the prior state of the document.
CREATE TABLE IF NOT EXISTS document_versions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  document_id INTEGER NOT NULL,
  version INTEGER NOT NULL,
  title TEXT NOT NULL,
  input_type TEXT NOT NULL,
  output_type TEXT NOT NULL,
  content TEXT NOT NULL,
  rendered_html TEXT,
  theme TEXT,
  custom_css TEXT,
  pdf_options TEXT,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_document_versions_doc ON document_versions(document_id, version);

-- Public share links. The token is the secret; the hash is stored. Optional
-- password and expiration. View count is updated on each successful access.
CREATE TABLE IF NOT EXISTS share_links (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  document_id INTEGER NOT NULL,
  user_id INTEGER NOT NULL,
  token_hash TEXT NOT NULL UNIQUE,
  prefix TEXT NOT NULL,
  format TEXT NOT NULL,                -- "html" | "pdf"
  expires_at INTEGER,
  password_hash TEXT,
  password_salt TEXT,
  view_count INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_share_links_doc ON share_links(document_id);
CREATE INDEX IF NOT EXISTS idx_share_links_user ON share_links(user_id);

-- Encryption-at-rest markers on documents. When `is_encrypted = 1` the
-- `content` column holds a base64 (`nonce.ciphertext`) blob encrypted with
-- AES-256-GCM under a key derived from the per-document password.
ALTER TABLE documents ADD COLUMN is_encrypted INTEGER NOT NULL DEFAULT 0;
ALTER TABLE documents ADD COLUMN encryption_salt TEXT;
