CREATE TABLE IF NOT EXISTS providers (
  id TEXT PRIMARY KEY NOT NULL,
  secondary BOOLEAN NOT NULL DEFAULT 1,
  enabled BOOLEAN NOT NULL DEFAULT 1,
  position INTEGER NOT NULL DEFAULT 999,
  added_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Map rows from `ron` string array representing `Vec<ProviderId>`
INSERT INTO providers (id, secondary, enabled, position)
SELECT DISTINCT
  trim(value),
  0,
  1,
  json_each.key
FROM settings, json_each(
  -- Transform unquoted strings to valid json
  '["' || replace(
      trim(settings.primary_providers, '[]'),
      ',',
      '","'
  ) || '"]'
);

INSERT INTO providers (id, secondary, enabled, position)
SELECT DISTINCT
  trim(value),
  1,
  1,
  json_each.key
FROM settings, json_each(
  '["' || replace(
      trim(settings.secondary_providers, '[]'),
      ',',
      '","'
  ) || '"]'
);

ALTER TABLE settings DROP COLUMN primary_providers;
ALTER TABLE settings DROP COLUMN secondary_providers;
