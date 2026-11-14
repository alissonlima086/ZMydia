-- Reclassifica itens já inspecionados que são quadrados/quase quadrados e
-- possuem menos de três minutos. Vídeos verticais já eram Shorts.
UPDATE media
SET is_short = 1,
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE is_short = 0
  AND duration_ms >= 0
  AND duration_ms < 180000
  AND width > 0
  AND height > 0
  AND CASE
        WHEN rotation % 180 IN (90, -90)
          THEN height * 10 <= width * 11
        ELSE width * 10 <= height * 11
      END;
