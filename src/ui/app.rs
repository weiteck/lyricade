use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use futures::stream::StreamExt;
use relm4::abstractions::Toaster;
use relm4::actions::AccelsPlus;
use relm4::actions::{RelmAction, RelmActionGroup};
use relm4::adw::prelude::*;
use relm4::tokio::sync::oneshot;
use relm4::tokio::task::AbortHandle;
use relm4::{RelmContainerExt, prelude::*};
use relm4_components::alert::{Alert, AlertMsg, AlertResponse, AlertSettings};
use tracing::{debug, error, trace, warn};

use crate::lyrics::LyricsType;
use crate::settings::{APP_ID, APP_NAME_PRETTY, CONNECTION_LIMIT};
use crate::ui::about::{AboutModel, AboutOutput};
use crate::ui::app::get_lyrics_menu::{
  GetLyricsButtonModel, GetLyricsButtonOutput, GetLyricsMenuState,
};
use crate::ui::prefs::{PrefsModel, PrefsOutput};
use crate::ui::tracks_table::{
  TracksTableFilter, TracksTableModel, TracksTableMsg, TracksTableOutput,
};
use crate::ui::view_lyrics::{ViewLyricsModel, ViewLyricsOutput, ViewLyricsSource};
use crate::{Result, library::Library, track::Track};
use crate::{SETTINGS, init_app, util};

mod get_lyrics_menu;

#[expect(clippy::struct_excessive_bools)]
struct AppModel {
  sender: AsyncComponentSender<Self>,
  libraries: Vec<Library>,
  tracks: Vec<Track>,
  track_stats: TrackStats,

  get_lyrics_button: Controller<GetLyricsButtonModel>,
  get_lyrics_menu_state: GetLyricsMenuState,

  tracks_table_widget: Controller<TracksTableModel>,
  prefs_widget: Option<Controller<PrefsModel>>,
  about_widget: Controller<AboutModel>,
  view_lyrics_widget: Option<Controller<ViewLyricsModel>>,
  confirm_get_lyrics_dialog: Controller<Alert>,
  confirm_clean_up_sidecar_files_dialog: Option<Controller<Alert>>,
  sidebar_widget: gtk::Box,
  search_entry: gtk::SearchEntry,
  toaster: Toaster,

  get_lyrics_requires_confirmation: bool,

  no_tracks: bool,
  track_count: u32,
  filtered_track_count: Option<u32>,
  filtered_track_ids: HashSet<i32>,

  selection_state: SelectionState,
  last_selection_state: SelectionState,
  selected_track_id: Option<i32>,
  selected_track_ids: HashSet<i32>,

  is_sidebar_pinned: bool,
  is_sidebar_revealed: bool,

  is_search_revealed: bool,
  search_query: Option<String>,
  active_search_filters: HashSet<TracksTableFilter>,

  is_fetching_lyrics: bool,
  fetch_lyrics_abort_handle: Option<AbortHandle>,

  is_cleaning_up_sidecar_files: bool,
  clean_up_sidecar_files_cancel_token: Option<oneshot::Sender<()>>,

  refresh_library_cancel_token: Option<oneshot::Sender<()>>,

  /// Name of the task being tracked.
  progress_task: Option<String>,
  /// The current step or state of the task.
  progress_step: Option<String>,
  progress: f64,

  /// Name of the task being tracked.
  spinner_task: Option<String>,
  /// The current step or state of the task.
  spinner_step: Option<String>,
}

#[derive(Debug)]
enum AppMsg {
  FetchLyrics,
  FetchLyricsComplete,
  CleanUpSidecarFiles,
  CleanUpSidecarFilesComplete,
  CancelOperation,
  /// Load libraries and tracks from the database.
  LoadLibraries,
  /// Scan library paths for changes.
  RefreshLibraries,
  /// Update the table with the tracks in `AppModel`.
  BuildTracksTable,
  Quit,

  ShowAboutWindow,
  CloseAboutWindow,
  ShowLyricsWindow(ViewLyricsSource),
  CloseLyricsWindow,
  ShowPrefsWindow,
  ClosePrefsWindow,

  GetLyricsMenuChanged(GetLyricsMenuState),
  RequestConfirmGetLyrics,
  HandleGetLyricsResponse(AlertResponse),

  RequestConfirmCleanUpSidecarFiles,
  HandleCleanUpSidecarFilesResponse(AlertResponse),

  ShowToast(String),

  ShowTrackDetailsSidebar,
  PinTrackDetailsSidebar(bool),
  TogglePinTrackDetailsSidebar,

  ShowSearch(bool),
  SearchQueryChanged(String),
  SetSearchFilter((TracksTableFilter, bool)),
  UpdateSelection(HashSet<i32>),
  UpdateFiltered(HashSet<i32>),

  RefreshTrackStats,

  ProgressStart(String),
  ProgressUpdate(ProgressUpdate),
  ProgressComplete,

  ShowSpinner((String, String)),
  HideSpinner,
}

#[derive(Debug)]
enum AppCommand {
  TrackUpdated(Track),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionState {
  None,
  Single,
  Multi,
}

#[derive(Debug)]
struct ProgressUpdate {
  step: Option<String>,
  progress: f64,
}

#[relm4::component(async)]
impl AsyncComponent for AppModel {
  type Input = AppMsg;
  type Output = ();
  type Init = ();
  type CommandOutput = AppCommand;

  view! {
    #[name(header_bar)]
    &adw::HeaderBar {
      pack_start = &gtk::ToggleButton {
        set_label: "Search",
        set_tooltip_text: Some("Search"),
        set_icon_name: "edit-find-symbolic",
        #[watch]
        set_active: model.is_search_revealed,
        connect_toggled[sender] => move |btn| {
          sender.input(AppMsg::ShowSearch(btn.is_active()));
        },
      },

      pack_end = &gtk::MenuButton {
        set_icon_name: "open-menu-symbolic",
        set_primary: true,
        set_tooltip: "Main Menu",
        set_menu_model: Some(&main_menu),
      },

      pack_end = &gtk::ToggleButton {
        set_tooltip_text: Some("Pin Track Details"),
        set_icon_name: "sidebar-show-right-symbolic",
        #[watch]
        set_sensitive: !model.no_tracks,
        #[watch]
        set_active: model.is_sidebar_pinned,
        connect_toggled[sender] => move |btn| {
          sender.input(AppMsg::PinTrackDetailsSidebar(btn.is_active()));
        },
      },

      // Cancel button shown if lyrics fetching in progress
      pack_end = &gtk::Button {
        set_label: "_Cancel",
        set_use_underline: true,
        set_tooltip_text: Some("Cancel Get Lyrics"),
        set_margin_end: 12,
        #[watch]
        set_visible: model.is_fetching_lyrics,
        connect_clicked => AppMsg::CancelOperation,
      },

      // Lyrics fetch button shown if fetching not in progress
      #[local_ref]
      pack_end = get_lyrics_button -> adw::SplitButton {
        #[watch]
        set_label: if model.get_lyrics_requires_confirmation { "_Get Lyrics…" } else { "_Get Lyrics" },
        set_use_underline: true,
        set_tooltip_text: Some("Get Lyrics from lrclib.net"),
        set_margin_end: 12,
        #[watch]
        set_visible: !model.is_fetching_lyrics,
        #[watch]
        set_class_active: ("suggested-action", !model.no_tracks),
        #[watch]
        set_sensitive: !model.no_tracks,
        connect_clicked => AppMsg::RequestConfirmGetLyrics,
      },
    },

    #[name(search_bar)]
    &gtk::SearchBar {
      #[watch]
      set_search_mode: model.is_search_revealed,
      set_key_capture_widget: Some(&main_window),
      connect_entry: search_entry,

      #[wrap(Some)]
      set_child = &gtk::Box {
        set_orientation: gtk::Orientation::Vertical,

        append = &adw::Clamp {
          set_maximum_size: 600,
          set_tightening_threshold: 400,

          #[local_ref]
          search_entry -> gtk::SearchEntry {
          set_hexpand: true,
          set_placeholder_text: Some("Type to search"),

          connect_search_changed[sender] => move |query| {
            sender.input(AppMsg::SearchQueryChanged(query.text().to_string()));
          },

          connect_stop_search => AppMsg::ShowSearch(false),
          }
        },

        // Filter chip buttons
        append = &gtk::ScrolledWindow {
          set_hscrollbar_policy: gtk::PolicyType::External,
          set_vscrollbar_policy: gtk::PolicyType::Never,
          set_hexpand: true,

          gtk::Box {
            set_orientation: gtk::Orientation::Horizontal,
            set_halign: gtk::Align::Center,
            set_margin_top: 8,
            set_margin_bottom: 4,

            gtk::ToggleButton {
              set_label: "no _lyrics",
              set_use_underline: true,
              set_hexpand: false,
              set_margin_end: 4,
              set_css_classes: &["pill", "caption"],
              inline_css: "padding: 0 0.75rem",
              #[watch]
              set_active: model.active_search_filters.contains(&TracksTableFilter::NoLyrics),
              connect_toggled[sender] => move |btn| {
                sender.input(AppMsg::SetSearchFilter((TracksTableFilter::NoLyrics, btn.is_active())));
              },
            },

            gtk::ToggleButton {
              set_label: "no lyrics _tag",
              set_use_underline: true,
              set_hexpand: false,
              set_margin_end: 4,
              set_css_classes: &["pill", "caption"],
              inline_css: "padding: 0 0.75rem",
              #[watch]
              set_active: model.active_search_filters.contains(&TracksTableFilter::NoLyricsTag),
              connect_toggled[sender] => move |btn| {
                sender.input(AppMsg::SetSearchFilter((TracksTableFilter::NoLyricsTag, btn.is_active())));
              },
            },

            gtk::ToggleButton {
              set_label: "l_rc file",
              set_use_underline: true,
              set_hexpand: false,
              set_margin_end: 4,
              set_css_classes: &["pill", "caption"],
              inline_css: "padding: 0 0.75rem",
              #[watch]
              set_active: model.active_search_filters.contains(&TracksTableFilter::Lrc),
              connect_toggled[sender] => move |btn| {
                sender.input(AppMsg::SetSearchFilter((TracksTableFilter::Lrc, btn.is_active())));
              },
            },

            gtk::ToggleButton {
              set_label: "t_xt file",
              set_use_underline: true,
              set_hexpand: false,
              set_margin_end: 4,
              set_css_classes: &["pill", "caption"],
              inline_css: "padding: 0 0.75rem",
              #[watch]
              set_active: model.active_search_filters.contains(&TracksTableFilter::Txt),
              connect_toggled[sender] => move |btn| {
                sender.input(AppMsg::SetSearchFilter((TracksTableFilter::Txt, btn.is_active())));
              },
            },

            gtk::ToggleButton {
              set_label: "not _sync",
              set_use_underline: true,
              set_hexpand: false,
              set_margin_end: 4,
              set_css_classes: &["pill", "caption"],
              inline_css: "padding: 0 0.75rem",
              #[watch]
              set_active: model.active_search_filters.contains(&TracksTableFilter::NotSync),
              connect_toggled[sender] => move |btn| {
                sender.input(AppMsg::SetSearchFilter((TracksTableFilter::NotSync, btn.is_active())));
              },
            },

            gtk::ToggleButton {
              set_label: "never _checked",
              set_use_underline: true,
              set_hexpand: false,
              set_margin_end: 4,
              set_css_classes: &["pill", "caption"],
              inline_css: "padding: 0 0.75rem",
              #[watch]
              set_active: model.active_search_filters.contains(&TracksTableFilter::NeverChecked),
              connect_toggled[sender] => move |btn| {
                sender.input(AppMsg::SetSearchFilter((TracksTableFilter::NeverChecked, btn.is_active())));
              },
            },

            gtk::ToggleButton {
              set_label: "not _instrumental",
              set_use_underline: true,
              set_hexpand: false,
              set_margin_end: 4,
              set_css_classes: &["pill", "caption"],
              inline_css: "padding: 0 0.75rem",
              #[watch]
              set_active: model.active_search_filters.contains(&TracksTableFilter::NotInstrumental),
              connect_toggled[sender] => move |btn| {
                sender.input(AppMsg::SetSearchFilter((TracksTableFilter::NotInstrumental, btn.is_active())));
              },
            },
          }
        }
      }
    },

    #[root]
    main_window = adw::ApplicationWindow {
      // Ensure settings are saved on close
      connect_close_request[sender] => move |_| {
        sender.input(AppMsg::Quit);
        gtk::glib::Propagation::Proceed
      },

      #[local_ref]
      toast_overlay -> adw::ToastOverlay {
        adw::ToolbarView {
          add_top_bar: &header_bar,
          add_top_bar: &search_bar,

          // Overlay for spinner
          gtk::Overlay {
            add_overlay = &gtk::Box {
              #[watch]
              set_visible: model.spinner_task.is_some(),
              set_orientation: gtk::Orientation::Vertical,
              set_align: gtk::Align::Fill,
              add_css_class: "window-bg-overlay",

              // Pad top for centring
              gtk::Box {
                set_vexpand: true,
              },

              gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 12,

                gtk::Spinner {
                  set_halign: gtk::Align::Center,
                  set_valign: gtk::Align::End,
                  set_size_request: (32, 32),
                  set_spinning: true,
                },

                gtk::Label {
                  set_halign: gtk::Align::Center,
                  set_valign: gtk::Align::Start,
                  add_css_class: "heading",
                  set_margin_top: 12,
                  #[watch]
                  set_label: &model.spinner_task.as_deref().unwrap_or(""),
                },

                gtk::Label {
                  set_halign: gtk::Align::Center,
                  set_valign: gtk::Align::Start,
                  set_justify: gtk::Justification::Center,
                  set_margin_bottom: 12,
                  #[watch]
                  set_label: &model.spinner_step.as_deref().unwrap_or(""),
                },

                gtk::Button {
                  set_halign: gtk::Align::Center,
                  set_valign: gtk::Align::Start,
                  set_label: "Cancel",
                  connect_clicked => AppMsg::CancelOperation,
                },
              },

              // Pad bottom for centring
              gtk::Box {
                set_vexpand: true,
              },
            },

            #[wrap(Some)]
            set_child = &gtk::Box {

              #[transition = "Crossfade"]
              match model.libraries.is_empty() {
                true => {
                  gtk::Box {
                    #[watch]
                    set_visible: model.spinner_task.is_none(),
                    set_align: gtk::Align::Center,

                    adw::StatusPage {
                      set_title: &format!("Welcome to {}", &APP_NAME_PRETTY),
                      set_description: Some("Add a Music Library to get started"),
                      set_icon_name: Some("lyricade-symbolic"),
                      set_width_request: 300,
                      #[wrap(Some)]
                      set_child = &gtk::Button {
                        set_label: "_Add Music Library…",
                        set_use_underline: true,
                        set_css_classes: &["pill", "suggested-action"],
                        connect_clicked => AppMsg::ShowPrefsWindow,
                      },
                    },
                  }
                }

                false => {
                  gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,

                    adw::OverlaySplitView {
                      #[watch]
                      set_show_sidebar: model.is_sidebar_revealed,
                      #[watch]
                      set_collapsed: !model.is_sidebar_pinned,
                      set_sidebar_position: gtk::PackType::End,
                      #[watch]
                      set_enable_hide_gesture: !model.is_sidebar_pinned,
                      set_sidebar_width_fraction: 0.5,

                      // Tracks table view
                      #[wrap(Some)]
                      #[local_ref]
                      set_content = tracks_table -> gtk::Overlay {},

                      // Sidebar
                      #[wrap(Some)]
                      set_sidebar = &gtk::ScrolledWindow {
                        set_hscrollbar_policy: gtk::PolicyType::Never,
                        add_css_class: "sidebar-pane",

                        #[name = "sidebar_viewport"]
                        gtk::Viewport {
                          set_width_request: 300,
                          #[watch]
                          set_child: Some(&model.sidebar_widget),
                        },
                      },
                    },

                    // Status bar
                    gtk::Box {
                      set_orientation: gtk::Orientation::Horizontal,
                      set_halign: gtk::Align::Fill,
                      set_valign: gtk::Align::Center,
                      set_hexpand: true,
                      set_margin_all: 6,
                      set_spacing: 12,

                      // Progress bar
                      gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_align: gtk::Align::Start,
                        set_valign: gtk::Align::Center,
                        set_hexpand: true,
                        set_spacing: 6,

                        #[watch]
                        set_visible: model.progress_task.is_some(),

                        // Cancel button
                        gtk::Button {
                          set_tooltip_text: Some("Cancel"),
                          set_icon_name: "window-close-symbolic",
                          set_css_classes: &["flat", "circular", "mini-cancel"],
                          #[watch]
                          set_visible: model.is_fetching_lyrics || model.is_cleaning_up_sidecar_files,
                          connect_clicked => AppMsg::CancelOperation,
                        },

                        gtk::Label {
                          add_css_class: "caption",
                          set_margin_end: 12, // added spacing
                          #[watch]
                          set_text: model.progress_task.as_deref().unwrap_or_default(),
                        },

                        gtk::ProgressBar {
                          set_halign: gtk::Align::Start,
                          set_valign: gtk::Align::Center,
                          set_ellipsize: gtk::pango::EllipsizeMode::End,
                          set_show_text: false,
                          set_margin_end: 6, // added spacing
                          #[watch]
                          set_fraction: model.progress,
                        },

                        gtk::Label {
                          add_css_class: "caption",
                          #[watch]
                          set_text: model.progress_step.as_deref().unwrap_or_default(),
                        }
                      },

                      // Right-side of status bar
                      gtk::Box {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_halign: gtk::Align::End,
                        set_valign: gtk::Align::Center,
                        set_hexpand: true,

                        // Track count and stats
                        gtk::MenuButton {
                          set_direction: gtk::ArrowType::Up,
                          set_css_classes: &["caption", "flat", "status-bar-button"],
                          set_tooltip: "Show Statistics",
                          set_use_underline: true,
                          #[watch]
                          set_label: &format!(
                            "{}{} _Tracks",
                            model.filtered_track_count.map(|n| format!("{n}/")).unwrap_or_default(),
                            model.track_count
                          ),
                          connect_activate => AppMsg::RefreshTrackStats,

                          #[wrap(Some)]
                          set_popover = &gtk::Popover {
                            set_position: gtk::PositionType::Top,

                            // Track stats
                            gtk::Box {
                              set_orientation: gtk::Orientation::Horizontal,
                              set_spacing: 6,

                              // Left column - value names
                              gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 6,

                                gtk::Label {
                                  set_align: gtk::Align::End,
                                  add_css_class: "heading",
                                  set_tooltip: "Not Marked “Instrumental”",
                                  #[watch]
                                  set_label: "Non-Inst.:",
                                },

                                gtk::Label {
                                  set_align: gtk::Align::End,
                                  add_css_class: "heading",
                                  set_tooltip: "Have Lyrics Tag",
                                  #[watch]
                                  set_label: "Tagged:",
                                },

                                gtk::Label {
                                  set_align: gtk::Align::End,
                                  add_css_class: "heading",
                                  set_tooltip: "Have Lyrics Sidecar File",
                                  #[watch]
                                  set_label: "Sidecar:",
                                },

                                gtk::Label {
                                  set_align: gtk::Align::End,
                                  add_css_class: "heading",
                                  set_tooltip: "Have Synchronous Lyrics",
                                  #[watch]
                                  set_label: "Sync:",
                                },

                                gtk::Label {
                                  set_align: gtk::Align::End,
                                  add_css_class: "heading",
                                  set_tooltip: "Have Plain Lyrics",
                                  #[watch]
                                  set_label: "Plain:",
                                },
                              },

                              // Right column - values
                              gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 6,

                                // "Non-Inst."
                                gtk::Label {
                                  set_align: gtk::Align::Start,
                                  set_tooltip: "Not Marked “Instrumental”",
                                  #[watch]
                                  set_label: &format!(
                                    "{}/{} ({} %)",
                                    model.track_stats.not_instrumental,
                                    model.track_stats.count,
                                    model.track_stats.not_instrumental_percent().round()
                                  ),
                                },

                                // "Tagged"
                                gtk::Label {
                                  set_align: gtk::Align::Start,
                                  set_tooltip: "Have Lyrics Tag",
                                  #[watch]
                                  set_label: &format!(
                                    "{}/{} ({} %)",
                                    model.track_stats.tagged_lyrics,
                                    model.track_stats.not_instrumental,
                                    model.track_stats.tagged_lyrics_percent().round()
                                  ),
                                },

                                // "Sidecar"
                                gtk::Label {
                                  set_align: gtk::Align::Start,
                                  set_tooltip: "Have Lyrics Sidecar File",
                                  #[watch]
                                  set_label: &format!(
                                    "{}/{} ({} %)",
                                    model.track_stats.sidecar_file,
                                    model.track_stats.not_instrumental,
                                    model.track_stats.sidecar_file_percent().round()
                                  ),
                                },

                                // "Sync"
                                gtk::Label {
                                  set_align: gtk::Align::Start,
                                  set_tooltip: "Have Synchronous Lyrics",
                                  #[watch]
                                  set_label: &format!(
                                    "{}/{} ({} %)",
                                    model.track_stats.sync_lyrics,
                                    model.track_stats.not_instrumental,
                                    model.track_stats.sync_lyrics_percent().round()
                                  ),
                                },

                                // "Plain"
                                gtk::Label {
                                  set_align: gtk::Align::Start,
                                  set_tooltip: "Have Plain Lyrics",
                                  #[watch]
                                  set_label: &format!(
                                    "{}/{} ({} %)",
                                    model.track_stats.plain_lyrics,
                                    model.track_stats.not_instrumental,
                                    model.track_stats.plain_lyrics_percent().round()
                                  ),
                                },
                              },
                            },
                          },
                        },
                      },
                    },
                  }
                },
              },
            },
          },
        },
      },
    },
  }

  menu! {
    main_menu: {
      "_Get Lyrics" => ActionFetchLyrics,
      section! {
        "_Refresh Libraries" => ActionRefreshLibraries,
        "_Clean Up Sidecar Files" => ActionCleanUpSidecarFiles,
      },
      section! {
        "_Preferences" => ActionPrefs,
        &format!("_About {APP_NAME_PRETTY}") => ActionAbout,
      },
      // section! {
      //   "_Debug" {
      //     "Test _Toast" => ActionTestToast,
      //     "Test _Spinner" => ActionTestSpinner,
      //   }
      // }
    }
  }

  async fn init(
    _init: Self::Init,
    root: Self::Root,
    sender: relm4::AsyncComponentSender<Self>,
  ) -> AsyncComponentParts<Self> {
    // Prepare logging, database and settings
    init_app().await.expect("Failed to initialise app");

    let get_lyrics_button =
      GetLyricsButtonModel::builder()
        .launch(())
        .forward(sender.input_sender(), |msg| match msg {
          GetLyricsButtonOutput::GetLyricsMenuChanged(state) => AppMsg::GetLyricsMenuChanged(state),
        });
    let get_lyrics_menu_state = get_lyrics_button.model().state();

    let tracks_table_widget =
      TracksTableModel::builder()
        .launch(())
        .forward(sender.input_sender(), |msg| match msg {
          TracksTableOutput::RowActivated => AppMsg::ShowTrackDetailsSidebar,
          TracksTableOutput::TrackIdsSelected(set) => AppMsg::UpdateSelection(set),
          TracksTableOutput::TrackIdsVisible(set) => AppMsg::UpdateFiltered(set),
        });

    let about_widget = AboutModel::builder()
      .launch(())
      .forward(sender.input_sender(), |msg| match msg {
        AboutOutput::Close => AppMsg::CloseAboutWindow,
      });

    let confirm_get_lyrics_dialog = Alert::builder()
      .transient_for(&root)
      .launch(AlertSettings {
        text: Some("Are you sure?".into()),
        secondary_text: Some("Tags will be written to your files.".into()),
        is_modal: true,
        destructive_accept: false,
        confirm_label: Some("Confirm".into()),
        cancel_label: Some("Cancel".into()),
        option_label: None,
        extra_child: None,
      })
      .forward(sender.input_sender(), AppMsg::HandleGetLyricsResponse);

    let mut model = AppModel {
      sender: sender.clone(),
      libraries: vec![],
      tracks: vec![],
      track_stats: TrackStats::default(),
      get_lyrics_button,
      get_lyrics_menu_state,
      tracks_table_widget,
      prefs_widget: None,
      about_widget,
      sidebar_widget: gtk::Box::new(gtk::Orientation::Vertical, 0),
      view_lyrics_widget: None,
      confirm_get_lyrics_dialog,
      confirm_clean_up_sidecar_files_dialog: None,
      search_entry: gtk::SearchEntry::new(),
      toaster: Toaster::default(),
      get_lyrics_requires_confirmation: true,
      no_tracks: false,
      track_count: 0,
      filtered_track_ids: HashSet::new(),
      filtered_track_count: None,
      is_search_revealed: false,
      search_query: None,
      selection_state: SelectionState::None,
      active_search_filters: HashSet::new(),
      last_selection_state: SelectionState::None,
      selected_track_id: None,
      selected_track_ids: HashSet::new(),
      is_sidebar_pinned: false,
      is_sidebar_revealed: false,
      is_fetching_lyrics: false,
      fetch_lyrics_abort_handle: None,
      is_cleaning_up_sidecar_files: false,
      clean_up_sidecar_files_cancel_token: None,
      refresh_library_cancel_token: None,
      progress_task: None,
      progress_step: None,
      progress: 0.0,
      spinner_task: None,
      spinner_step: None,
    };

    model.refresh_from_settings(&root, &sender);

    // References used in `view` macro
    let get_lyrics_button = model.get_lyrics_button.widget();
    let toast_overlay = model.toaster.overlay_widget();
    let tracks_table = model.tracks_table_widget.widget();
    let search_entry = &model.search_entry;

    let widgets = view_output!();

    // Set window title
    let app_title = if cfg!(debug_assertions) {
      format!("{APP_NAME_PRETTY} (DEBUG)")
    } else {
      APP_NAME_PRETTY.to_string()
    };
    widgets.main_window.set_title(Some(&app_title));

    // Restore previous window configuration
    let (width, height, is_sidebar_pinned) = if let Ok(guard) = SETTINGS.read() {
      (
        guard.window_width.clamp(400, 3840),
        guard.window_height.clamp(400, 3840),
        guard.sidebar_pinned,
      )
    } else {
      (800, 600, false)
    };
    widgets.main_window.set_default_size(width, height);

    if is_sidebar_pinned {
      model.is_sidebar_pinned = is_sidebar_pinned;
      model.rebuild_sidebar_widget();
    }

    // Load libraries and tracks and populate table view
    sender.input(AppMsg::LoadLibraries);

    // Main menu actions
    relm4::new_action_group!(pub MainMenuActionGroup, "main_menu_action_group");
    let mut menu_actions_group = RelmActionGroup::<MainMenuActionGroup>::new();

    relm4::new_stateless_action!(ActionRefreshLibraries, MainMenuActionGroup, "refresh_libraries");
    let action_refresh_libraries: RelmAction<ActionRefreshLibraries> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::RefreshLibraries);
      })
    };
    menu_actions_group.add_action(action_refresh_libraries);

    relm4::new_stateless_action!(ActionFetchLyrics, MainMenuActionGroup, "fetch_lyrics");
    let action_fetch_lyrics: RelmAction<ActionFetchLyrics> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::RequestConfirmGetLyrics);
      })
    };
    menu_actions_group.add_action(action_fetch_lyrics);

    relm4::new_stateless_action!(
      ActionCleanUpSidecarFiles,
      MainMenuActionGroup,
      "clean_up_sidecar_files"
    );
    let action_clean_up_sidecar_files: RelmAction<ActionCleanUpSidecarFiles> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::RequestConfirmCleanUpSidecarFiles);
      })
    };
    menu_actions_group.add_action(action_clean_up_sidecar_files);

    relm4::new_stateless_action!(ActionPrefs, MainMenuActionGroup, "prefs");
    let action_prefs: RelmAction<ActionPrefs> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::ShowPrefsWindow);
      })
    };
    menu_actions_group.add_action(action_prefs);

    relm4::new_stateless_action!(ActionAbout, MainMenuActionGroup, "about");
    let action_about: RelmAction<ActionAbout> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::ShowAboutWindow);
      })
    };
    menu_actions_group.add_action(action_about);

    relm4::new_stateless_action!(ActionTestToast, MainMenuActionGroup, "test_toast");
    let action_test_toast: RelmAction<ActionTestToast> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::ShowToast("Testing toast notification".into()));
      })
    };
    menu_actions_group.add_action(action_test_toast);

    relm4::new_stateless_action!(ActionTestSpinner, MainMenuActionGroup, "test_spinner");
    let action_test_spinner: RelmAction<ActionTestSpinner> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender
          .input(AppMsg::ShowSpinner(("I'm spinning around…".into(), "Get out of my way".into())));
      })
    };
    menu_actions_group.add_action(action_test_spinner);

    // Keyboard actions
    relm4::new_action_group!(pub WindowActionGroup, "window_action_group");
    let mut window_actions_group = RelmActionGroup::<WindowActionGroup>::new();

    relm4::new_stateless_action!(ActionQuit, WindowActionGroup, "quit");
    let action_quit: RelmAction<ActionQuit> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::Quit);
      })
    };
    window_actions_group.add_action(action_quit);

    relm4::new_stateless_action!(ActionSearch, WindowActionGroup, "search");
    let action_search: RelmAction<ActionSearch> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::ShowSearch(true));
      })
    };
    window_actions_group.add_action(action_search);

    relm4::new_stateless_action!(ActionPinSidebar, WindowActionGroup, "pin_sidebar");
    let pin_sidebar: RelmAction<ActionPinSidebar> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::TogglePinTrackDetailsSidebar);
      })
    };
    window_actions_group.add_action(pin_sidebar);

    // Keyboard shortcuts
    let app = relm4::main_adw_application();
    app.set_accelerators_for_action::<ActionSearch>(&["<primary>f"]);
    app.set_accelerators_for_action::<ActionPinSidebar>(&["F9"]);
    app.set_accelerators_for_action::<ActionRefreshLibraries>(&["<primary>r"]);
    app.set_accelerators_for_action::<ActionPrefs>(&["<primary>comma"]);
    app.set_accelerators_for_action::<ActionQuit>(&["<primary>q"]);

    // Register menu/keyboard actions for main window
    menu_actions_group.register_for_widget(&widgets.main_window);
    window_actions_group.register_for_widget(&widgets.main_window);

    AsyncComponentParts { model, widgets }
  }

  async fn update(
    &mut self,
    message: Self::Input,
    sender: AsyncComponentSender<Self>,
    root: &Self::Root,
  ) {
    match message {
      AppMsg::GetLyricsMenuChanged(state) => {
        debug!("Get Lyrics menu state updated: {:#?}", &state);
        self.get_lyrics_menu_state = state;
      }

      AppMsg::RequestConfirmGetLyrics => {
        // Show confirmation dialog only if tags will be written
        if self.get_lyrics_requires_confirmation {
          debug!("Showing Get Lyrics confirmation alert");
          self.confirm_get_lyrics_dialog.emit(AlertMsg::Show);
        } else {
          sender.input(AppMsg::FetchLyrics);
        }
      }

      AppMsg::HandleGetLyricsResponse(response) => {
        if let AlertResponse::Confirm = response {
          debug!("User confirmed Get Lyrics request");
          sender.input(AppMsg::FetchLyrics);
        } else {
          debug!("User cancelled Get Lyrics request");
        }
      }

      AppMsg::RequestConfirmCleanUpSidecarFiles => {
        debug!("Showing CleanUpSidecarFiles confirmation alert");
        self
          .confirm_clean_up_sidecar_files_dialog
          .as_ref()
          .inspect(|alert| alert.emit(AlertMsg::Show));
      }

      AppMsg::HandleCleanUpSidecarFilesResponse(response) => {
        if let AlertResponse::Confirm = response {
          debug!("User confirmed CleanUpSidecarFiles request");
          sender.input(AppMsg::CleanUpSidecarFiles);
        } else {
          debug!("User cancelled CleanUpSidecarFiles request");
        }
      }

      #[expect(clippy::cast_possible_truncation)]
      AppMsg::FetchLyrics => {
        self.is_fetching_lyrics = true;

        // Display progress
        sender.input(AppMsg::ProgressStart("Getting lyrics…".into()));
        sender.input(AppMsg::ProgressUpdate(ProgressUpdate {
          step: Some(format!("0 / {} done", self.track_count)),
          progress: 0.0,
        }));

        let preferred_lyrics_type = SETTINGS
          .read()
          .map_or(LyricsType::Sync, |settings| settings.prefer_lyrics_type);
        let filtered_tracks = self
          .tracks
          .iter()
          .filter(|track| {
            self
              .get_lyrics_menu_state
              .filter_track(track, preferred_lyrics_type)
          })
          .cloned()
          .collect::<Vec<_>>();

        let total = self.tracks.len();
        let completed = Arc::new(AtomicUsize::new(0));
        let stream = futures::stream::iter(filtered_tracks);
        let batch_size = (CONNECTION_LIMIT as f64 * 1.5) as usize;

        // Batch process tracks and update progress
        let jh = relm4::spawn(async move {
          stream
            .for_each_concurrent(batch_size, |mut track| {
              let sender = sender.clone();
              let completed = Arc::clone(&completed);

              async move {
                let _ = track.fetch_lyrics().call().await;

                let completed = completed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                sender
                  .command_sender()
                  .emit(AppCommand::TrackUpdated(track));

                sender.input(AppMsg::ProgressUpdate(ProgressUpdate {
                  step: Some(format!("{completed} / {total} done")),
                  progress: completed as f64 / total as f64,
                }));
              }
            })
            .await;

          // End display progress
          sender.input(AppMsg::FetchLyricsComplete);
        });

        self.fetch_lyrics_abort_handle = Some(jh.abort_handle());
      }

      AppMsg::FetchLyricsComplete => {
        debug!("FetchLyrics completed");
        self.fetch_lyrics_abort_handle = None;
        self.is_fetching_lyrics = false;
        self.update_track_stats();
        sender.input(AppMsg::ProgressComplete);
      }

      AppMsg::CleanUpSidecarFiles => {
        self.is_cleaning_up_sidecar_files = true;

        let total = self.tracks.len();
        let tracks = self.tracks.clone();

        // Display progress
        sender.input(AppMsg::ProgressStart("Cleaning up sidecar files…".into()));
        sender.input(AppMsg::ProgressUpdate(ProgressUpdate {
          step: Some(format!("0 / {} done", total)),
          progress: 0.0,
        }));

        let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
        self.clean_up_sidecar_files_cancel_token = Some(cancel_tx);

        // Process tracks in background thread and update progress
        relm4::spawn_blocking(move || {
          let sender = sender.clone();

          for (idx, mut track) in tracks.into_iter().enumerate() {
            // Cancel operation if sender was dropped
            if cancel_rx
              .try_recv()
              .is_err_and(|error| error == oneshot::error::TryRecvError::Closed)
            {
              break;
            }

            let _ = track.clean_up_sidecar_files().call();

            sender
              .command_sender()
              .emit(AppCommand::TrackUpdated(track));

            let completed = idx + 1;
            if completed.is_multiple_of(5) {
              sender.input(AppMsg::ProgressUpdate(ProgressUpdate {
                step: Some(format!("{completed} / {total} done")),
                progress: completed as f64 / total as f64,
              }));
            }
          }

          // End display progress
          sender.input(AppMsg::CleanUpSidecarFilesComplete);
        });
      }

      AppMsg::CleanUpSidecarFilesComplete => {
        debug!("CleanUpSidecarFiles completed");
        self.clean_up_sidecar_files_cancel_token = None;
        self.is_cleaning_up_sidecar_files = false;
        self.update_track_stats();
        sender.input(AppMsg::ProgressComplete);
      }

      // Cancel either fetching lyrics, clean up sidecar files, or refresh libraries operations
      AppMsg::CancelOperation => {
        // Abort the async fetch lyrics task
        if let Some(handle) = self.fetch_lyrics_abort_handle.take() {
          handle.abort();
          debug!("FetchLyrics cancelled by user");
          sender.input(AppMsg::ProgressComplete);
        }

        // Drop the sender to cancel the clean up task
        if self.clean_up_sidecar_files_cancel_token.take().is_some() {
          debug!("CleanUpSidecarFiles cancelled by user");
          sender.input(AppMsg::ProgressComplete);
        }

        // Drop the sender to cancel the clean up task
        if self.refresh_library_cancel_token.take().is_some() {
          self.spinner_task = None;
          self.spinner_step = None;
          debug!("RefreshLibraries cancelled by user");
        }

        self.is_fetching_lyrics = false;
        self.is_cleaning_up_sidecar_files = false;

        self.update_track_stats();
      }

      AppMsg::ShowAboutWindow => {
        debug!("Showing About window");
        let window = self.about_widget.widget();
        window.set_transient_for(Some(root));
        window.set_hide_on_close(true);
        window.present();
      }

      AppMsg::CloseAboutWindow => {
        debug!("Closing About window");
        self.about_widget.widget().close();
      }

      AppMsg::ShowPrefsWindow => {
        debug!("Showing Preferences window");

        if let Ok(guard) = SETTINGS.read() {
          let settings = guard.clone();
          drop(guard);

          let prefs_widget = PrefsModel::builder()
            .launch((settings, self.libraries.clone()))
            .forward(sender.input_sender(), |msg| match msg {
              PrefsOutput::Close => AppMsg::ClosePrefsWindow,
            });

          let window = prefs_widget.widget();
          window.set_transient_for(Some(root));
          window.set_hide_on_close(true);
          window.present();

          self.prefs_widget = Some(prefs_widget);
        } else {
          sender.input(AppMsg::ShowToast("Cannot read settings".into()));
        }
      }

      AppMsg::ClosePrefsWindow => {
        debug!("Closing Preferences window");

        self
          .prefs_widget
          .as_ref()
          .inspect(|ctrl| ctrl.widget().close());
        self.prefs_widget = None;

        let current_libs = Library::get_all().expect("failed to get Libraries");
        let new_libs = current_libs
          .iter()
          .filter(|&lib| !self.libraries.contains(lib))
          .cloned()
          .collect::<Vec<_>>();
        let libs_have_been_removed = self.libraries.iter().any(|lib| !current_libs.contains(lib));

        self.libraries = current_libs;

        // Scan newly-added libraries
        if !new_libs.is_empty() {
          debug!("Libraries have been added; refreshing");

          let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
          self.refresh_library_cancel_token = Some(cancel_tx);

          let sender_handle = sender.clone();
          relm4::spawn_blocking(move || {
            for lib in new_libs {
              if cancel_rx
                .try_recv()
                .is_err_and(|error| error == oneshot::error::TryRecvError::Closed)
              {
                break;
              }

              let name = lib.name();
              let progress_sender = sender_handle.clone();
              let progress_callback = move |msg| {
                progress_sender
                  .input(AppMsg::ShowSpinner((format!("Scanning new library “{name}”…"), msg)));
              };

              let _ = lib
                .refresh()
                .on_progress(progress_callback.clone())
                .cancel_on_close(&mut cancel_rx)
                .call()
                .inspect_err(|error| warn!("{error}"));
            }

            sender_handle.input(AppMsg::LoadLibraries);
            sender_handle.input(AppMsg::HideSpinner);
          });
        } else if libs_have_been_removed {
          debug!("Libraries have been deleted; refreshing");

          sender.input(AppMsg::LoadLibraries);
        } else {
          // Refresh table if no changes to libraries in case datetime format changed
          sender.input(AppMsg::BuildTracksTable);
        }

        self.refresh_from_settings(root, &sender);
      }

      AppMsg::ShowLyricsWindow(source) => {
        // Close any existing window
        self
          .view_lyrics_widget
          .as_ref()
          .inspect(|ctrl| ctrl.widget().close());
        self.view_lyrics_widget = None;

        if let Some(track) = self
          .selected_track_id
          .and_then(|idx| self.tracks.iter().find(|track| track.id == idx))
        {
          debug!("Showing ViewLyrics window with lyrics type \"{source:?}\" for {track}");

          let controller = ViewLyricsModel::builder()
            .launch((Box::new(track.clone()), source))
            .forward(sender.input_sender(), |msg| match msg {
              ViewLyricsOutput::Close => AppMsg::CloseLyricsWindow,
            });

          let window = controller.widget();
          window.set_transient_for(Some(root));
          window.present();

          self.view_lyrics_widget = Some(controller);
        } else {
          error!("Tried to show ViewLyrics window but could not reference Track");
        }
      }

      AppMsg::CloseLyricsWindow => {
        debug!("Closing ViewLyrics window");
        self
          .view_lyrics_widget
          .as_ref()
          .inspect(|ctrl| ctrl.widget().close());
        self.view_lyrics_widget = None;
      }

      // TODO: Use alert dialog to show errors
      AppMsg::LoadLibraries => {
        // Can clear the channel as refresh is done at this point
        self.refresh_library_cancel_token = None;

        if self
          .load_libraries()
          .inspect_err(|e| {
            sender.input(AppMsg::ShowToast(format!("Error loading music libraries: {e}")));
          })
          .is_ok()
        {
          // Update the table view
          sender.input(AppMsg::BuildTracksTable);

          self.no_tracks = self.tracks.is_empty();

          // Reset track selection state
          self.selected_track_id = None;
          self.selected_track_ids.clear();
          self.update_selection_state();

          if !self.no_tracks {
            sender.input(AppMsg::ShowToast(format!(
              "Loaded {} music {} with {} {}",
              self.libraries.len(),
              if self.libraries.len() <= 1 {
                "library"
              } else {
                "libraries"
              },
              self.tracks.len(),
              if self.tracks.len() == 1 {
                "track"
              } else {
                "tracks"
              }
            )));
          }
        }
      }

      AppMsg::BuildTracksTable => self
        .tracks_table_widget
        .sender()
        .emit(TracksTableMsg::ClearAndAppend(self.tracks.clone())),

      AppMsg::RefreshLibraries => {
        let libs = self.libraries.clone();
        let sender_handle = sender.clone();

        let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
        self.refresh_library_cancel_token = Some(cancel_tx);

        relm4::spawn_blocking(move || {
          for lib in libs {
            if cancel_rx
              .try_recv()
              .is_err_and(|error| error == oneshot::error::TryRecvError::Closed)
            {
              break;
            }

            let name = lib.name();
            let progress_sender = sender_handle.clone();
            let progress_callback = move |msg| {
              progress_sender
                .input(AppMsg::ShowSpinner((format!("Refreshing library “{name}”"), msg)));
            };

            let _ = lib
              .refresh()
              .on_progress(progress_callback.clone())
              .cancel_on_close(&mut cancel_rx)
              .call()
              .inspect_err(|error| warn!("{error}"));
          }

          sender_handle.input(AppMsg::LoadLibraries);
          sender_handle.input(AppMsg::HideSpinner);
        });
      }

      AppMsg::SearchQueryChanged(query) => {
        debug!("Searching for: {}", &query);
        self.is_search_revealed = true;
        self.search_query = if query.is_empty() { None } else { Some(query) };

        self
          .tracks_table_widget
          .sender()
          .emit(TracksTableMsg::Filter(self.search_query.clone()));
      }

      AppMsg::ShowToast(msg) => {
        debug!("Emit toast notification: \"{}\"", &msg);
        let toast = adw::Toast::builder().title(msg).timeout(3).build();
        self.toaster.add_toast(toast);
      }

      AppMsg::ShowSearch(active) => {
        if active {
          debug!("Search bar revealed");

          self.search_entry.grab_focus();
        } else {
          debug!("Search bar hidden");

          self.search_query = None;

          self
            .tracks_table_widget
            .sender()
            .emit(TracksTableMsg::ClearFilters);

          self.active_search_filters.clear();
        }

        self.is_search_revealed = active;
      }

      AppMsg::SetSearchFilter((filter, active)) => {
        debug!("Search filter \"{:?}\" active: {}", &filter, active);

        let transformed_filter = match filter {
          TracksTableFilter::Lrc
            if self.active_search_filters.contains(&TracksTableFilter::Txt) =>
          {
            // Restore other filter if one becomes inactive
            if !active {
              self
                .tracks_table_widget
                .sender()
                .emit(TracksTableMsg::SetFilter((TracksTableFilter::Txt, true)));
            }
            TracksTableFilter::EitherLrcOrTxt
          }

          TracksTableFilter::Txt
            if self.active_search_filters.contains(&TracksTableFilter::Lrc) =>
          {
            // Restore other filter if one becomes inactive
            if !active {
              self
                .tracks_table_widget
                .sender()
                .emit(TracksTableMsg::SetFilter((TracksTableFilter::Lrc, true)));
            }
            TracksTableFilter::EitherLrcOrTxt
          }

          _ => filter,
        };

        if active {
          self.active_search_filters.insert(filter);
        } else {
          self.active_search_filters.remove(&filter);
        }

        self
          .tracks_table_widget
          .sender()
          .emit(TracksTableMsg::SetFilter((transformed_filter, active)));
      }

      AppMsg::ShowTrackDetailsSidebar => {
        debug!("Showing sidebar");
        self.is_sidebar_revealed = true;
        self.rebuild_sidebar_widget();
      }

      AppMsg::PinTrackDetailsSidebar(active) => {
        if !self.no_tracks {
          debug!("Pinning sidebar: {active}");
          if active && self.is_sidebar_revealed {
            self.rebuild_sidebar_widget();
          }
          self.is_sidebar_pinned = active;
          self.is_sidebar_revealed = active;
        }
      }

      AppMsg::TogglePinTrackDetailsSidebar => {
        debug!("Toggling pin sidebar");

        sender.input(AppMsg::PinTrackDetailsSidebar(!self.is_sidebar_pinned));
      }

      AppMsg::UpdateSelection(set) => {
        self.selected_track_ids = set;
        debug!("Tracks selected: {}", self.selected_track_ids.len());
        trace!("Selected Track IDs:\n{:#?}", self.selected_track_ids);

        // Selection changed; hide track details unless pinned
        if !self.is_sidebar_pinned {
          self.is_sidebar_revealed = false;
        }

        self.update_selection_state();
      }

      #[expect(clippy::cast_possible_truncation)]
      AppMsg::UpdateFiltered(set) => {
        debug!("Updating list and count of filtered tracks");

        let count = set.len() as u32;
        if count == self.track_count {
          debug!("No tracks filtered");
          self.filtered_track_count = None;
        } else {
          debug!("Filtered Track Count: {count}");
          self.filtered_track_count = Some(count);

          // Unselect tracks not in filtered set
          self.selected_track_ids.retain(|id| set.contains(id));
          self.update_selection_state();
        }

        self.filtered_track_ids = set;
      }

      AppMsg::RefreshTrackStats => {
        debug!("Refreshing TrackStats");

        self
          .track_stats
          .refresh_from_filtered(&self.filtered_track_ids);
      }

      AppMsg::ProgressStart(task_name) => {
        debug!("Progress task start: \"{task_name}\"");
        self.progress_task = Some(task_name);
      }

      AppMsg::ProgressComplete => {
        debug!(
          "Progress task complete: \"{}\"",
          &self.progress_task.as_deref().unwrap_or_default()
        );
        self.progress_task = None;
      }

      AppMsg::ProgressUpdate(pu) => {
        debug!(
          "Progress task update: {:02} % of task \"{}\" at step \"{:?}\"",
          &pu.progress * 100.0,
          &self.progress_task.as_deref().unwrap_or_default(),
          &pu.step
        );

        if self.progress_step != pu.step {
          self.progress_step = pu.step;
        }
        self.progress = pu.progress;
      }

      AppMsg::ShowSpinner((task, step)) => {
        debug!("Showing spinner: \"{task}\", \"{step}\"");
        self.spinner_task = Some(task);
        self.spinner_step = Some(step);
      }

      AppMsg::HideSpinner => {
        debug!("Hiding spinner");
        self.spinner_task = None;
      }

      AppMsg::Quit => {
        // Save window size
        let (width, height) = (root.default_width(), root.default_height());
        if let Ok(mut guard) = SETTINGS.write() {
          guard.window_width = width;
          guard.window_height = height;
          guard.sidebar_pinned = self.is_sidebar_pinned;
          debug!("Persisted window size {width}x{height} and sidebar pin state to Settings");
          let _ = guard.save();
        } else {
          error!("");
        }

        gtk::glib::idle_add_local_once(move || {
          relm4::main_adw_application().quit();
        });
      }
    }
  }

  async fn update_cmd(
    &mut self,
    message: Self::CommandOutput,
    _sender: AsyncComponentSender<Self>,
    _root: &Self::Root,
  ) {
    match message {
      AppCommand::TrackUpdated(track) => {
        // Replace local copy
        if let Some(t) = self
          .tracks
          .iter()
          .position(|t| t.id == track.id)
          .and_then(|idx| self.tracks.get_mut(idx))
        {
          *t = track.clone();
        }

        // Update table
        self
          .tracks_table_widget
          .sender()
          .emit(TracksTableMsg::Update(Box::new(track)));
      }
    }
  }
}

impl AppModel {
  fn load_libraries(&mut self) -> Result<()> {
    debug!("Loading Libraries and Tracks ...");

    self.libraries = Library::get_all()?;
    self.load_tracks();

    debug!("Loaded {} Libraries", self.libraries.len());

    Ok(())
  }

  #[expect(clippy::cast_possible_truncation)]
  fn load_tracks(&mut self) {
    debug!("Loading Tracks from {} Libraries ...", self.libraries.len());

    self.tracks = self
      .libraries
      .iter()
      .filter_map(|lib| {
        lib
          .tracks()
          .call()
          .inspect_err(|e| error!("Error getting tracks for Library {}: {e}", lib))
          .ok()
      })
      .flatten()
      .collect::<Vec<_>>();

    self.track_count = self.tracks.len() as u32;
    self.update_track_stats();

    debug!("Loaded {} Tracks from {} Libraries", self.tracks.len(), self.libraries.len());

    self.no_tracks = self.tracks.is_empty();
  }

  fn update_track_stats(&mut self) {
    self.track_stats.update(&self.tracks);
  }

  /// Update flags and dialogs based on current `Settings`.
  fn refresh_from_settings(
    &mut self,
    root: &adw::ApplicationWindow,
    sender: &AsyncComponentSender<AppModel>,
  ) {
    if let Ok(guard) = SETTINGS.read() {
      self.get_lyrics_requires_confirmation = guard.update_lyrics_tag_on_fetch;

      let secondary_text = if guard.upgrade_lyrics_tag_on_scan
        && (guard.delete_sidecar_files_on_scan || guard.keep_one_sidecar_file_on_scan)
      {
        String::from("Sidecar files will be deleted and tags will be written to your files.")
      } else if guard.delete_sidecar_files_on_scan || guard.keep_one_sidecar_file_on_scan {
        String::from("Sidecar files will be deleted.")
      } else if guard.upgrade_lyrics_tag_on_scan {
        String::from("Lyrics tags will be written to your files.")
      } else {
        String::from("This will have no effect with the options set in Preferences.")
      };

      drop(guard);

      let confirm_clean_up_sidecar_files_dialog = Alert::builder()
        .transient_for(root)
        .launch(AlertSettings {
          text: Some("Are you sure?".into()),
          secondary_text: Some(secondary_text),
          is_modal: true,
          destructive_accept: true,
          confirm_label: Some("Confirm".into()),
          cancel_label: Some("Cancel".into()),
          option_label: None,
          extra_child: None,
        })
        .forward(sender.input_sender(), AppMsg::HandleCleanUpSidecarFilesResponse);

      self.confirm_clean_up_sidecar_files_dialog = Some(confirm_clean_up_sidecar_files_dialog);
    }
  }

  #[expect(clippy::cast_possible_truncation)]
  fn rebuild_sidebar_widget(&mut self) {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 24);
    root.set_margin_all(12);
    root.set_margin_top(0);

    if let Some(track) = self
      .selected_track_id
      .and_then(|id| self.tracks.iter().find(|t| t.id == id))
    {
      debug!("Building sidebar with {track} selected");

      root.set_halign(gtk::Align::Fill);

      // Helper function for button to view lyrics
      let view_lyrics_button = |src| {
        let btn = gtk::Button::new();
        btn.add_css_class("flat");
        btn.set_valign(gtk::Align::Center);
        btn.set_icon_name("document-text-symbolic");
        btn.set_tooltip("View Lyrics");
        let sender = self.sender.clone();
        btn.connect_clicked(move |_| {
          sender.input(AppMsg::ShowLyricsWindow(src));
        });
        btn
      };

      // General track info
      let pg = adw::PreferencesGroup::new();
      pg.set_title("Track Details");

      let ar = adw::ActionRow::new();
      ar.set_title("Artist Name");
      ar.set_subtitle(&track.artist_name);
      ar.set_use_markup(false);
      pg.container_add(&ar);

      let ar = adw::ActionRow::new();
      ar.set_title("Album Title");
      ar.set_subtitle(&track.album_name);
      ar.set_use_markup(false);
      pg.container_add(&ar);

      let ar = adw::ActionRow::new();
      ar.set_title("Track Title");
      ar.set_subtitle(&track.track_name);
      ar.set_use_markup(false);
      pg.container_add(&ar);

      let ar = adw::ActionRow::new();
      ar.set_title("Duration");
      let duration = track.duration.round();
      ar.set_subtitle(&format!("{}:{:02}", duration as u32 / 60, duration as u32 % 60));
      ar.set_use_markup(false);
      pg.container_add(&ar);

      let ar = adw::ActionRow::new();
      ar.set_title("File Date");
      ar.set_subtitle(&util::ndt_utc_to_ui_string(track.file_modified_at));
      ar.set_use_markup(false);
      pg.container_add(&ar);

      let ar = adw::ActionRow::new();
      ar.set_title("Path");
      ar.set_subtitle(&track.path);
      ar.set_use_markup(false);
      pg.container_add(&ar);

      root.append(&pg);

      // Lyrics info
      let inner = gtk::Box::new(gtk::Orientation::Vertical, 12);

      let pg = adw::PreferencesGroup::new();
      pg.set_title("Lyrics");

      let ar = adw::ActionRow::new();
      ar.set_title("Lyrics Tag");
      if track.lyrics.is_some() {
        let btn = view_lyrics_button(ViewLyricsSource::Tag);
        ar.add_suffix(&btn);
        ar.set_subtitle("Present");
      } else {
        ar.set_subtitle("Missing");
      }
      pg.container_add(&ar);

      if track.lyrics.is_some() {
        let ar = adw::ActionRow::new();
        ar.set_title("Type");
        ar.set_subtitle(if track.lyrics_synchronised {
          "Synchronised"
        } else {
          "Plain"
        });
        pg.container_add(&ar);
      }

      inner.append(&pg);

      if track.lyrics_sidecar_lrc_file.is_some() {
        let pg = adw::PreferencesGroup::new();

        let ar = adw::ActionRow::new();
        ar.set_title("Sidecar File");
        ar.set_subtitle("LRC format");

        let btn = view_lyrics_button(ViewLyricsSource::Lrc);
        ar.add_suffix(&btn);

        pg.container_add(&ar);
        inner.append(&pg);
      }

      if track.lyrics_sidecar_txt_file.is_some() {
        let pg = adw::PreferencesGroup::new();

        let ar = adw::ActionRow::new();
        ar.set_title("Sidecar File");
        ar.set_subtitle("TXT format");

        let btn = view_lyrics_button(ViewLyricsSource::Txt);
        ar.add_suffix(&btn);

        pg.container_add(&ar);
        inner.append(&pg);
      }

      if track.instrumental.is_some_and(|b| b) {
        let pg = adw::PreferencesGroup::new();

        let ar = adw::ActionRow::new();
        ar.set_title("Instrumental");
        ar.set_subtitle("True");

        pg.container_add(&ar);
        inner.append(&pg);
      }

      let pg = adw::PreferencesGroup::new();

      let ar = adw::ActionRow::new();
      ar.set_title("Last Check for Lyrics");
      ar.set_subtitle(
        &track
          .last_api_check_at
          .map_or_else(|| "Never".into(), util::ndt_utc_to_ui_string),
      );

      pg.container_add(&ar);
      inner.append(&pg);

      root.append(&inner);

      // Extended debugging info
      if cfg!(debug_assertions) {
        let pg = adw::PreferencesGroup::new();
        pg.set_title("Debug Information");

        let ar = adw::ActionRow::new();
        ar.set_title("Track Id");
        ar.set_subtitle(&track.id.to_string());
        pg.container_add(&ar);

        let ar = adw::ActionRow::new();
        ar.set_title("Library Id");
        ar.set_subtitle(&track.library_id.to_string());
        pg.container_add(&ar);

        let ar = adw::ActionRow::new();
        ar.set_title("Added At");
        ar.set_subtitle(&util::ndt_utc_to_ui_string(track.added_at));
        pg.container_add(&ar);

        let ar = adw::ActionRow::new();
        ar.set_title("Updated At");
        ar.set_subtitle(&util::ndt_utc_to_ui_string(track.updated_at));
        pg.container_add(&ar);

        let ar = adw::ActionRow::new();
        ar.set_title("Refreshed At");
        ar.set_subtitle(&util::ndt_utc_to_ui_string(track.refreshed_at));
        pg.container_add(&ar);

        root.append(&pg);
      }
    } else if self.selected_track_ids.len() > 1 {
      debug!("Building sidebar with multiple tracks selected");
      root.set_valign(gtk::Align::Center);

      let selected = self.selected_track_ids.len();

      let status_page = adw::StatusPage::new();
      status_page.set_title(&format!("{selected} tracks selected"));
      status_page.set_description(Some("Select one track to view details"));
      status_page.set_icon_name(Some("music-queue-symbolic"));
      status_page.add_css_class("compact");

      root.append(&status_page);
    } else {
      debug!("Building sidebar with no track selected");
      root.set_valign(gtk::Align::Center);

      let status_page = adw::StatusPage::new();
      status_page.set_title("No track selected");
      status_page.set_description(Some("Select a track to view details"));
      status_page.set_icon_name(Some("lyricade-symbolic"));
      status_page.add_css_class("compact");

      root.append(&status_page);
    }

    self.sidebar_widget = root;
  }

  fn update_selection_state(&mut self) {
    match self.selected_track_ids.len() {
      0 => {
        self.selected_track_id = None;
        self.change_selection_state(SelectionState::None);
      }
      1 => {
        self.selected_track_id = self.selected_track_ids.iter().next().copied();
        self.change_selection_state(SelectionState::Single);
      }
      _ => {
        self.selected_track_id = None;
        self.change_selection_state(SelectionState::Multi);
      }
    }
  }

  fn change_selection_state(&mut self, new_state: SelectionState) {
    if self.selection_state != new_state {
      self.last_selection_state = self.selection_state;
      self.selection_state = new_state;
    }

    if self.is_sidebar_revealed {
      self.rebuild_sidebar_widget();
    }
  }
}

#[derive(Debug, Clone, Default)]
struct TrackStats {
  instrumental_set: HashSet<i32>,
  not_instrumental_set: HashSet<i32>,
  never_checked_set: HashSet<i32>,
  sync_lyrics_set: HashSet<i32>,
  plain_lyrics_set: HashSet<i32>,
  tagged_lyrics_set: HashSet<i32>,
  sidecar_file_set: HashSet<i32>,

  count: usize,
  instrumental: usize,
  not_instrumental: usize,
  never_checked: usize,
  sync_lyrics: usize,
  plain_lyrics: usize,
  tagged_lyrics: usize,
  sidecar_file: usize,
}

impl TrackStats {
  fn update(&mut self, tracks: &[Track]) {
    trace!("Building TrackStats");

    *self = Self::default();

    self.count = tracks.len();

    for track in tracks {
      if track.last_api_check_at.is_none() {
        self.never_checked_set.insert(track.id);
      }

      if track.instrumental.is_some_and(|b| b) {
        self.instrumental_set.insert(track.id);
      } else {
        self.not_instrumental_set.insert(track.id);

        if track.lyrics_synchronised || track.lyrics_sidecar_lrc_file.is_some() {
          self.sync_lyrics_set.insert(track.id);
        }

        if !track.lyrics_synchronised
          && (track.lyrics.is_some() || track.lyrics_sidecar_txt_file.is_some())
        {
          self.plain_lyrics_set.insert(track.id);
        }

        if track.lyrics.is_some() {
          self.tagged_lyrics_set.insert(track.id);
        }

        if track.lyrics_sidecar_lrc_file.is_some() || track.lyrics_sidecar_txt_file.is_some() {
          self.sidecar_file_set.insert(track.id);
        }
      }
    }

    self.instrumental = self.instrumental_set.len();
    self.not_instrumental = self.not_instrumental_set.len();
    self.never_checked = self.never_checked_set.len();
    self.sync_lyrics = self.sync_lyrics_set.len();
    self.plain_lyrics = self.plain_lyrics_set.len();
    self.tagged_lyrics = self.tagged_lyrics_set.len();
    self.sidecar_file = self.sidecar_file_set.len();
  }

  fn refresh_from_filtered(&mut self, track_ids: &HashSet<i32>) {
    self.instrumental = self.instrumental_set.intersection(track_ids).count();
    self.not_instrumental = self.not_instrumental_set.intersection(track_ids).count();
    self.never_checked = self.never_checked_set.intersection(track_ids).count();
    self.sync_lyrics = self.sync_lyrics_set.intersection(track_ids).count();
    self.plain_lyrics = self.plain_lyrics_set.intersection(track_ids).count();
    self.tagged_lyrics = self.tagged_lyrics_set.intersection(track_ids).count();
    self.sidecar_file = self.sidecar_file_set.intersection(track_ids).count();
  }

  fn not_instrumental_percent(&self) -> f64 {
    (self.not_instrumental as f64 / self.count as f64) * 100.0
  }

  fn sync_lyrics_percent(&self) -> f64 {
    (self.sync_lyrics as f64 / self.not_instrumental as f64) * 100.0
  }

  fn plain_lyrics_percent(&self) -> f64 {
    (self.plain_lyrics as f64 / self.not_instrumental as f64) * 100.0
  }

  fn tagged_lyrics_percent(&self) -> f64 {
    (self.tagged_lyrics as f64 / self.not_instrumental as f64) * 100.0
  }

  fn sidecar_file_percent(&self) -> f64 {
    (self.sidecar_file as f64 / self.not_instrumental as f64) * 100.0
  }
}

pub fn start() {
  let app = RelmApp::new(APP_ID);

  // Custom icons
  initialize_custom_icons();

  // Inject CSS
  let css = include_str!("../../data/style.css");
  let provider = gtk::CssProvider::new();
  provider.load_from_string(css);
  gtk::style_context_add_provider_for_display(
    &gtk::gdk::Display::default().expect("could not connect to display"),
    &provider,
    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
  );

  app.run_async::<AppModel>(());
}

fn initialize_custom_icons() {
  gtk::gio::resources_register_include!("resources.gresource")
    .expect("failed to include gresources");

  let display = gtk::gdk::Display::default().expect("could not connect to display");
  let theme = gtk::IconTheme::for_display(&display);
  theme.add_resource_path("/io/github/weiteck/Lyricade/icons");
}
