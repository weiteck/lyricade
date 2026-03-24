slint::include_modules!();

use std::{cell::RefCell, rc::Rc, sync::LazyLock};

use camino::Utf8PathBuf;
use slint::{Model, ModelRc, StandardListViewItem, ToSharedString, VecModel};
use tracing::{debug, info};

use crate::{Result, library::Library, track::Track, util};

pub static DIRECTORY_PICKER_START_DIR: LazyLock<Utf8PathBuf> = LazyLock::new(|| {
    directories::UserDirs::new()
        .map(|ud| Utf8PathBuf::from_path_buf(ud.home_dir().to_path_buf()).ok())
        .flatten()
        .unwrap_or_else(|| Utf8PathBuf::from("/"))
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

pub struct TracksModel {
    pub tracks: Rc<RefCell<Vec<Track>>>,
    pub indices: VecModel<usize>,
}

impl Model for TracksModel {
    type Data = ModelRc<StandardListViewItem>;

    fn row_count(&self) -> usize {
        self.indices.row_count()
    }

    fn row_data(&self, row: usize) -> Option<Self::Data> {
        let track_idx = self.indices.row_data(row)?;
        let tracks = self.tracks.borrow();
        let track = tracks.get(track_idx)?;

        let checkmark = '✓'.to_shared_string();
        let blank = "".to_shared_string();

        let row = vec![
            StandardListViewItem::from(track.artist_name.to_shared_string()),
            StandardListViewItem::from(track.album_name.to_shared_string()),
            StandardListViewItem::from(track.track_name.to_shared_string()),
            StandardListViewItem::from(if track.instrumental.is_some_and(|inst| inst) {
                checkmark.clone()
            } else {
                blank.clone()
            }),
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
                    .unwrap_or(blank.clone()),
            ),
            StandardListViewItem::from(
                util::ndt_utc_to_local_dt(track.file_modified_at)
                    .format("%x %X")
                    .to_shared_string(),
            ),
            StandardListViewItem::from(track.path.to_shared_string()),
        ];

        Some(ModelRc::from(Rc::new(VecModel::from(row))))
    }

    fn model_tracker(&self) -> &dyn slint::ModelTracker {
        self.indices.model_tracker()
    }
}

pub struct State {
    pub libraries: Vec<Library>,
    pub tracks_model: Rc<TracksModel>,
    pub sort_key: SortKey,
    pub sort_direction: SortDirection,
    pub filter_query: String,
}

impl State {
    pub fn new() -> Result<Self> {
        let libraries = Library::get_all()?;
        let tracks = libraries
            .iter()
            .filter_map(|l| l.tracks().call().ok())
            .flatten()
            .collect::<Vec<_>>();
        let indices = tracks
            .iter()
            .enumerate()
            .map(|(idx, _)| idx)
            .collect::<VecModel<_>>();
        let tracks_model = Rc::new(TracksModel {
            tracks: Rc::new(RefCell::new(tracks)),
            indices,
        });
        let mut state = State {
            libraries,
            tracks_model,
            sort_key: SortKey::Path,
            sort_direction: SortDirection::Ascending,
            filter_query: String::new(),
        };
        state.sort();
        Ok(state)
    }

    pub fn set_sort(&mut self, key: SortKey, direction: SortDirection) {
        self.sort_key = key;
        self.sort_direction = direction;
        self.sort();
    }

    pub fn set_filter(&mut self, query: &str) {
        self.filter_query = query.to_lowercase().into();
        self.filter();
    }

    fn sort(&mut self) {
        {
            let mut tracks = self.tracks_model.tracks.borrow_mut();

            match self.sort_key {
                SortKey::Artist => tracks.sort_by_cached_key(|t| t.artist_name.to_lowercase()),
                SortKey::Album => tracks.sort_by_cached_key(|t| t.album_name.to_lowercase()),
                SortKey::Track => tracks.sort_by_cached_key(|t| t.track_name.to_lowercase()),
                SortKey::Instrumental => {
                    tracks.sort_by_cached_key(|t| t.instrumental.is_some_and(|inst| inst))
                }
                SortKey::Lyrics => tracks.sort_by_cached_key(|t| t.lyrics.is_some()),
                SortKey::LyricsSync => tracks.sort_by_cached_key(|t| t.lyrics_synchronised),
                SortKey::Lrc => tracks.sort_by_cached_key(|t| t.lyrics_sidecar_lrc_file.is_some()),
                SortKey::Txt => tracks.sort_by_cached_key(|t| t.lyrics_sidecar_txt_file.is_some()),
                SortKey::LastChecked => {
                    tracks.sort_by_cached_key(|t| t.last_api_check_at.unwrap_or_default())
                }
                SortKey::LastModified => tracks.sort_by_cached_key(|t| t.file_modified_at),
                SortKey::Path => tracks.sort_by_cached_key(|t| t.path()),
            }

            if self.sort_direction == SortDirection::Descending {
                tracks.reverse();
            }

            let indices = tracks
                .iter()
                .enumerate()
                .map(|(idx, _)| idx)
                .collect::<Vec<_>>();
            self.tracks_model.indices.set_vec(indices);
        }

        self.filter();
    }

    fn filter(&mut self) {
        let tracks = self.tracks_model.tracks.borrow_mut();

        let filtered_indices = tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                t.artist_name.to_lowercase().contains(&self.filter_query)
                    || t.album_name.to_lowercase().contains(&self.filter_query)
                    || t.track_name.to_lowercase().contains(&self.filter_query)
            })
            .map(|(idx, _)| idx)
            .collect::<Vec<_>>();

        self.tracks_model.indices.set_vec(filtered_indices);
    }
}

pub fn start() -> Result<()> {
    let main_window = MainWindow::new()?;

    let state = Rc::new(RefCell::new(State::new()?));

    main_window.set_track_table_rows(ModelRc::from(state.borrow().tracks_model.clone()));

    {
        // let tm = tracks_model.clone();
        main_window.on_button_test(move || {
            // if let Ok(tracks) = tm.tracks.borrow_mut() {
            //     for mut track in tracks {
            //         track.artist_name = String::from("CHANGED");
            //     }
            // }
        });
    }

    {
        let tm = state.borrow().tracks_model.clone();
        main_window.on_show_settings(move || {
            let data: Vec<usize> = tm.indices.iter().collect();
            let data = data
                .into_iter()
                .enumerate()
                .filter(|(i, _)| i % 2 == 0)
                .map(|(_, u)| u)
                .collect::<Vec<_>>();
            info!("{} remain", &data.len());
            tm.indices.set_vec(data);
        });
    }

    {
        let state = state.clone();
        main_window.on_sort_asc(move |col| {
            state
                .borrow_mut()
                .set_sort(SortKey::from(col), SortDirection::Ascending);
        });
    }
    {
        let state = state.clone();
        main_window.on_sort_desc(move |col| {
            state
                .borrow_mut()
                .set_sort(SortKey::from(col), SortDirection::Descending);
        });
    }
    {
        let state = state.clone();
        main_window.on_filter(move |text| {
            state.borrow_mut().set_filter(&text);
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
