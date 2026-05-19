-- URL watches: convert-when-it-changes pipelines.
--
-- Background tick polls each enabled watch on its schedule, fetches the
-- source URL, hashes the body, and — if the hash differs from the last
-- one we saw — runs a conversion (using `input_type` / `output_format`)
-- and POSTs the result to `target_url`. If `target_secret` is set the
-- body is HMAC-SHA256 signed and the digest is sent in the
-- `X-UDC-Signature` header (mirroring the existing batch webhook
-- behaviour).
--
-- `last_seen_hash` / `last_seen_at` / `last_delivered_at` / `last_error`
-- give the dashboard enough state to render a status pill without
-- replaying history.
CREATE TABLE IF NOT EXISTS url_watches (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  name TEXT NOT NULL,
  url TEXT NOT NULL,
  input_type TEXT NOT NULL,        -- markdown | html | json | xml | csv | org | asciidoc | rst | latex
  output_format TEXT NOT NULL,     -- html | pdf
  target_url TEXT NOT NULL,
  target_secret TEXT,
  poll_interval_secs INTEGER NOT NULL DEFAULT 900,  -- 15 minutes default
  enabled INTEGER NOT NULL DEFAULT 1,
  last_seen_hash TEXT,
  last_seen_at INTEGER,
  last_polled_at INTEGER,
  last_delivered_at INTEGER,
  last_error TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Worker queries `WHERE enabled = 1 AND (last_polled_at IS NULL OR last_polled_at + poll_interval_secs <= now)`
-- on every tick; this composite index is what makes that cheap at scale.
CREATE INDEX IF NOT EXISTS idx_url_watches_due
  ON url_watches(enabled, last_polled_at);
CREATE INDEX IF NOT EXISTS idx_url_watches_user
  ON url_watches(user_id, created_at);
