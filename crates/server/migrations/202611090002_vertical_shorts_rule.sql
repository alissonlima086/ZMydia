-- Update media classification for all vertical videos regardless of duration
UPDATE media
SET is_short = 1
WHERE (
    (rotation IS NOT NULL AND (abs(rotation) % 180 = 90) AND width > height)
    OR
    ((rotation IS NULL OR (abs(rotation) % 180 = 0)) AND width < height)
) AND is_short = 0;
