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
  lyrics_synchronised BOOLEAN NOT NULL,
  lyrics_sidecar_lrc_file TEXT,
  lyrics_sidecar_txt_file TEXT,
  added_at DATETIME NOT NULL,
  updated_at DATETIME NOT NULL,
  refreshed_at DATETIME NOT NULL,
  last_api_check_at DATETIME,
  file_modified_at DATETIME NOT NULL,
  FOREIGN KEY (library_id)
    REFERENCES libraries (id)
      ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tracks_library_id ON tracks (library_id);

CREATE TABLE IF NOT EXISTS settings (
  -- Singleton table
  id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),

  prefer_accurate_timestamps BOOLEAN NOT NULL,
  scan_new_files_only BOOLEAN NOT NULL,
  plain_lyrics_in_id3v2_uslt_frame BOOLEAN NOT NULL,

  -- Fetching lyrics
  prefer_lyrics_type TEXT NOT NULL CHECK (prefer_lyrics_type IN ('Sync', 'Plain')) DEFAULT 'Sync',
  ignore_plain_lyrics_on_fetch BOOLEAN NOT NULL,
  update_lyrics_tag_on_fetch BOOLEAN NOT NULL,
  save_sidecar_file_on_fetch BOOLEAN NOT NULL DEFAULT 1,

  get_lyrics_menu_lyrics_type TEXT NOT NULL DEFAULT 'NotPreferred',
  get_lyrics_menu_last_checked TEXT NOT NULL DEFAULT 'Any',
  get_lyrics_menu_target_visible BOOLEAN NOT NULL,
  get_lyrics_menu_target_selected BOOLEAN NOT NULL,

  -- GUI
  window_width INTEGER NOT NULL DEFAULT 1000,
  window_height INTEGER NOT NULL DEFAULT 600,
  sidebar_pinned BOOLEAN NOT NULL,

  added_at DATETIME NOT NULL DEFAULT 'now',
  updated_at DATETIME NOT NULL DEFAULT 'now'
);
