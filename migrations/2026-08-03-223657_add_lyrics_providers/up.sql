ALTER TABLE settings ADD COLUMN primary_providers TEXT NOT NULL DEFAULT '[LrcLib]';
ALTER TABLE settings ADD COLUMN secondary_providers TEXT NOT NULL DEFAULT '[SimpMusic]';
