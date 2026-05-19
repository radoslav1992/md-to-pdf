-- Public template gallery: premium users can mark a template public, and
-- anyone (even free / anonymous) can browse the gallery. Cloning a
-- public template into your own library is premium-only (templates are
-- a premium feature). `clone_source_id` is a soft pointer so the
-- gallery view can show "Cloned from <name>".
--
-- We keep the simple BOOLEAN-as-INTEGER convention SQLite uses
-- throughout this codebase.
ALTER TABLE templates ADD COLUMN is_public INTEGER NOT NULL DEFAULT 0;
ALTER TABLE templates ADD COLUMN clone_source_id INTEGER;

CREATE INDEX IF NOT EXISTS idx_templates_public
  ON templates(is_public, updated_at)
  WHERE is_public = 1;
