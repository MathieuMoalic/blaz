ALTER TABLE recipes ADD COLUMN share_token TEXT;
CREATE UNIQUE INDEX recipes_share_token_uidx ON recipes (share_token) WHERE share_token IS NOT NULL;
