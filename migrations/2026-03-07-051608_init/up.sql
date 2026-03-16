CREATE TABLE IF NOT EXISTS libraries (
  -- `NOT NULL` added to PKs to help diesel generate schema
  id INTEGER PRIMARY KEY NOT NULL,
  path TEXT NOT NULL UNIQUE,
  name TEXT,
  added_at DATETIME NOT NULL,
  updated_at DATETIME NOT NULL
);

CREATE TABLE IF NOT EXISTS tracks (
  id INTEGER PRIMARY KEY NOT NULL,
  library_id INTEGER NOT NULL,
  path TEXT NOT NULL UNIQUE,
  track_name TEXT NOT NULL,
  artist_name TEXT NOT NULL,
  album_name TEXT NOT NULL,
  duration REAL NOT NULL,
  instrumental BOOLEAN,
  lyrics TEXT,
  lyrics_sidecar_lrc_file TEXT,
  lyrics_sidecar_txt_file TEXT,
  lyrics_embedded_synchronised BOOLEAN NOT NULL,
  added_at DATETIME NOT NULL,
  updated_at DATETIME NOT NULL,
  refreshed_at DATETIME NOT NULL,
  last_api_check_at DATETIME,
  file_modified_at DATETIME NOT NULL,
  FOREIGN KEY (library_id)
    REFERENCES libraries (id)
      ON DELETE CASCADE
      ON UPDATE CASCADE
);

CREATE INDEX idx_tracks_library_id ON tracks (library_id);
