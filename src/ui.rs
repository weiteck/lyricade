slint::include_modules!();

use std::{rc::Rc, sync::LazyLock};

use camino::Utf8PathBuf;
use slint::{ModelRc, SharedString, StandardListViewItem, ToSharedString, VecModel};
use tracing::{debug, info};

use crate::{Result, library::Library, track::Track, util};

pub static DIRECTORY_PICKER_START_DIR: LazyLock<Utf8PathBuf> = LazyLock::new(|| {
    directories::UserDirs::new()
        .map(|ud| Utf8PathBuf::from_path_buf(ud.home_dir().to_path_buf()).ok())
        .flatten()
        .unwrap_or_else(|| Utf8PathBuf::from("/"))
});

pub enum SortDirection {
    Ascending,
    Descending,
}

pub enum SortKey {
    Artist,
    Album,
    Track,
    Instrumental,
    Lyrics,
    LyricsSync,
    Lrc,
    Txt,
    LastChecked,
    LastModified,
    Path,
}

impl From<i32> for SortKey {
    fn from(col: i32) -> Self {
        match col {
            0 => Self::Artist,
            1 => Self::Album,
            2 => Self::Track,
            3 => Self::Instrumental,
            4 => Self::Lyrics,
            5 => Self::LyricsSync,
            6 => Self::Lrc,
            7 => Self::Txt,
            8 => Self::LastChecked,
            9 => Self::LastModified,
            _ => Self::Path,
        }
    }
}

#[derive(Clone)]
pub struct State {
    pub libraries: Vec<Library>,
    pub tracks: Vec<Track>,
}

impl State {
    pub fn new() -> Result<Self> {
        let libraries = Library::get_all()?;
        let mut tracks = libraries
            .iter()
            .filter_map(|l| l.tracks().call().ok())
            .flatten()
            .collect::<Vec<_>>();
        tracks.sort_by_cached_key(|t| t.path());
        Ok(State { libraries, tracks })
    }

    pub fn sorted_track_table_rows(
        &mut self,
        key: SortKey,
        direction: SortDirection,
    ) -> Rc<VecModel<ModelRc<StandardListViewItem>>> {
        match key {
            SortKey::Artist => self
                .tracks
                .sort_by_cached_key(|t| t.artist_name.to_lowercase()),
            SortKey::Album => self
                .tracks
                .sort_by_cached_key(|t| t.album_name.to_lowercase()),
            SortKey::Track => self
                .tracks
                .sort_by_cached_key(|t| t.track_name.to_lowercase()),
            SortKey::Instrumental => self
                .tracks
                .sort_by_cached_key(|t| t.instrumental.is_some_and(|inst| inst)),
            SortKey::Lyrics => self.tracks.sort_by_cached_key(|t| t.lyrics.is_some()),
            SortKey::LyricsSync => self.tracks.sort_by_cached_key(|t| t.lyrics_synchronised),
            SortKey::Lrc => self
                .tracks
                .sort_by_cached_key(|t| t.lyrics_sidecar_lrc_file.is_some()),
            SortKey::Txt => self
                .tracks
                .sort_by_cached_key(|t| t.lyrics_sidecar_txt_file.is_some()),
            SortKey::LastChecked => self
                .tracks
                .sort_by_cached_key(|t| t.last_api_check_at.unwrap_or_default()),
            SortKey::LastModified => self.tracks.sort_by_cached_key(|t| t.file_modified_at),
            SortKey::Path => self.tracks.sort_by_cached_key(|t| t.path()),
        }

        match direction {
            SortDirection::Ascending => self.track_table_rows(),
            SortDirection::Descending => {
                self.tracks.reverse();
                self.track_table_rows()
            }
        }
    }

    pub fn track_table_rows(&self) -> Rc<VecModel<ModelRc<StandardListViewItem>>> {
        let checkmark = '✓'.to_shared_string();
        let blank = "".to_shared_string();
        Rc::new(
            self.tracks
                .iter()
                .map(|track| {
                    let row = vec![
                        StandardListViewItem::from(track.artist_name.to_shared_string()),
                        StandardListViewItem::from(track.album_name.to_shared_string()),
                        StandardListViewItem::from(track.track_name.to_shared_string()),
                        StandardListViewItem::from(
                            if track.instrumental.is_some_and(|inst| inst) {
                                checkmark.clone()
                            } else {
                                blank.clone()
                            },
                        ),
                        StandardListViewItem::from(if track.lyrics.is_some() {
                            checkmark.clone()
                        } else {
                            blank.clone()
                        }),
                        StandardListViewItem::from(if track.lyrics_synchronised {
                            checkmark.clone()
                        } else {
                            blank.clone()
                        }),
                        StandardListViewItem::from(if track.lyrics_sidecar_lrc_file.is_some() {
                            checkmark.clone()
                        } else {
                            blank.clone()
                        }),
                        StandardListViewItem::from(if track.lyrics_sidecar_txt_file.is_some() {
                            checkmark.clone()
                        } else {
                            blank.clone()
                        }),
                        StandardListViewItem::from(
                            track
                                .last_api_check_at
                                .map(|ndt| {
                                    util::ndt_utc_to_local_dt(ndt)
                                        .format("%x %X")
                                        .to_shared_string()
                                })
                                .unwrap_or(SharedString::new()),
                        ),
                        StandardListViewItem::from(
                            util::ndt_utc_to_local_dt(track.file_modified_at)
                                .format("%x %X")
                                .to_shared_string(),
                        ),
                        StandardListViewItem::from(track.path.to_shared_string()),
                    ];
                    ModelRc::from(Rc::new(VecModel::from(row)))
                })
                .collect(),
        )
    }
}

pub fn start() -> Result<()> {
    let main_window = MainWindow::new()?;

    let state = State::new()?;

    main_window.set_track_table_rows(ModelRc::from(state.track_table_rows()));
    main_window.set_track_count(state.tracks.len() as i32);

    {
        let window = main_window.clone_strong();
        let mut state = state.clone();
        main_window.on_sort_asc(move |col| {
            let rows = state.sorted_track_table_rows(SortKey::from(col), SortDirection::Ascending);
            window.set_track_table_rows(ModelRc::from(rows));
        });
    }

    {
        let window = main_window.clone_strong();
        let mut state = state.clone();
        main_window.on_sort_desc(move |col| {
            let rows = state.sorted_track_table_rows(SortKey::from(col), SortDirection::Descending);
            window.set_track_table_rows(ModelRc::from(rows));
        });
    }

    let font_family = font_kit::source::SystemSource::new()
        .select_best_match(
            &[font_kit::family_name::FamilyName::SansSerif],
            &font_kit::properties::Properties::default(),
        )
        .map(|handle| match handle.load() {
            Ok(font) => font.family_name(),
            Err(_) => String::from("sans-serif"),
        })
        .unwrap_or_else(|_| String::from("sans-serif"));
    info!("Using system font-family: {}", &font_family);
    main_window.set_system_font_family(font_family.into());

    main_window.on_show_settings(|| {
        let window = SettingsWindow::new().expect("Failed to create Settings window");
        let handle = window.clone_strong();
        window.on_close(move || {
            handle.hide().expect("Failed to close Settings window");
        });
        window.show().expect("Failed to show Settings window");
    });

    // main_window.show()?;
    // slint::run_event_loop()?;
    main_window.run()?;

    Ok(())
}

pub fn directory_picker_dialog() -> Option<Utf8PathBuf> {
    rfd::FileDialog::new()
        .set_directory(&*DIRECTORY_PICKER_START_DIR)
        .pick_folder()
        .map(|pb| Utf8PathBuf::from_path_buf(pb).ok())
        .flatten()
        .inspect(|p| debug!("Directory picker: User selected \"{}\"", p))
}
