ALTER TABLE settings ADD COLUMN primary_providers TEXT NOT NULL DEFAULT '[LrcLib]';
ALTER TABLE settings ADD COLUMN secondary_providers TEXT NOT NULL DEFAULT '[SimpMusic,Genius]';

-- Reconstruct `ron` string value of `Vec<ProviderId>`
UPDATE settings
SET primary_providers = (
  SELECT '[' || group_concat(id, ',') || ']'
  FROM providers
  WHERE
    secondary = 0
    AND enabled = 1
  ORDER BY position
  );

UPDATE settings
SET secondary_providers = (
  SELECT '[' || group_concat(id, ',') || ']'
  FROM providers
  WHERE
    secondary = 1
    AND enabled = 1
  ORDER BY position
  );

DROP TABLE IF EXISTS providers;
