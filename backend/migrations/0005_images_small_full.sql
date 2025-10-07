ALTER TABLE recipes ADD COLUMN image_path_small TEXT;
ALTER TABLE recipes ADD COLUMN image_path_full  TEXT;

-- Backfill for existing rows so old data continues to work
UPDATE recipes
SET image_path_small = COALESCE(image_path_small, image_path),
    image_path_full  = COALESCE(image_path_full,  image_path);

