-- Premium styling fields on saved documents. All nullable so existing rows
-- and free-tier users (who can't set them) remain valid.

ALTER TABLE documents ADD COLUMN theme TEXT;
ALTER TABLE documents ADD COLUMN custom_css TEXT;
ALTER TABLE documents ADD COLUMN pdf_options TEXT;
