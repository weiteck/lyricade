use std::collections::HashSet;

use camino::Utf8PathBuf;
use relm4::abstractions::Toaster;
use relm4::actions::AccelsPlus;
use relm4::actions::{RelmAction, RelmActionGroup};
use relm4::adw::prelude::*;
use relm4::*;
use tracing::{debug, error, trace};

use crate::settings::APP_NAME_PRETTY;
use crate::ui::about::AboutModel;
use crate::ui::prefs::{PrefsModel, PrefsOutput};
use crate::ui::tracks_table::{
  TracksTableFilter, TracksTableModel, TracksTableMsg, TracksTableOutput,
};
use crate::util;
use crate::{Result, library::Library, track::Track};

pub struct AppModel {
  libraries: Vec<Library>,
  tracks: Vec<Track>,

  tracks_table_widget: Controller<TracksTableModel>,
  prefs_widget: Controller<PrefsModel>,
  about_widget: Controller<AboutModel>,
  sidebar_widget: gtk::Box,
  toaster: Toaster,

  no_tracks: bool,

  selection_state: SelectionState,
  last_selection_state: SelectionState,
  selected_track_id: Option<i32>,
  selected_track_ids: HashSet<i32>,

  is_sidebar_pinned: bool,
  is_sidebar_revealed: bool,

  is_search_revealed: bool,
  search_query: Option<String>,
}

#[derive(Debug)]
pub enum AppMsg {
  AddLibrary(Utf8PathBuf),
  FetchLyrics,
  Quit,
  /// Load libraries and tracks from the database.
  LoadLibraries,
  /// Scan library paths for changes.
  RefreshLibraries,
  /// Update the table with the tracks in `AppModel`.
  BuildTracksTable,
  ShowAbout,
  SearchQueryChanged(String),
  ShowSearch(bool),
  ShowPrefsWindow,
  ShowToast(String),
  SetSearchFilter((TracksTableFilter, bool)),
  ShowTrackDetailsSidebar,
  HideTrackDetailsSidebar,
  PinTrackDetailsSidebar(bool),
  UpdateSelection(HashSet<i32>),
}

#[derive(Debug)]
pub enum AppCommand {
  TrackUpdated(Track),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionState {
  None,
  Single,
  Multi,
}

#[relm4::component(pub)]
impl Component for AppModel {
  type Input = AppMsg;
  type Output = ();
  type Init = ();
  type CommandOutput = AppCommand;

  view! {
    #[name(header_bar)]
    &adw::HeaderBar {
      pack_start = &gtk::ToggleButton {
        set_label: "Search",
        set_tooltip_text: Some("Filter Tracks"),
        set_icon_name: "edit-find-symbolic",
        connect_toggled[sender] => move |btn| {
          sender.input(AppMsg::ShowSearch(btn.is_active()));
        },
      },

      pack_end = &gtk::MenuButton {
        set_icon_name: "open-menu-symbolic",
        set_primary: true,
        set_menu_model: Some(&main_menu),
      },

      pack_end = &gtk::ToggleButton {
        set_label: "Info",
        set_tooltip_text: Some("Pin Track Details"),
        set_icon_name: "info-outline-symbolic",
        #[watch]
        set_visible: !model.no_tracks,
        connect_toggled[sender] => move |btn| {
          sender.input(AppMsg::PinTrackDetailsSidebar(btn.is_active()));
        },
      },
    },

    #[name(search_bar)]
    &gtk::SearchBar {
      #[watch]
      set_search_mode: model.is_search_revealed,
      set_key_capture_widget: Some(&main_window),
      connect_entry: &search_entry,

      #[wrap(Some)]
      set_child = &gtk::Box {
        set_orientation: gtk::Orientation::Vertical,

        append = &adw::Clamp {
          set_maximum_size: 600,
          set_tightening_threshold: 400,

          #[name(search_entry)]
          gtk::SearchEntry {
          set_hexpand: true,
          set_placeholder_text: Some("Type to search"),

          connect_search_changed[sender] => move |query| {
              sender.input(AppMsg::SearchQueryChanged(query.text().to_string()))
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
              set_label: "lrc file",
              set_hexpand: false,
              set_margin_end: 4,
              set_css_classes: &["pill", "caption"],
              inline_css: "padding: 0 0.75rem",
              connect_toggled[sender] => move |btn| {
                  sender.input(AppMsg::SetSearchFilter((TracksTableFilter::Lrc, btn.is_active())));
              },
            },

            gtk::ToggleButton {
              set_label: "txt file",
              set_hexpand: false,
              set_margin_end: 4,
              set_css_classes: &["pill", "caption"],
              inline_css: "padding: 0 0.75rem",
              connect_toggled[sender] => move |btn| {
                  sender.input(AppMsg::SetSearchFilter((TracksTableFilter::Txt, btn.is_active())));
              },
            },

            gtk::ToggleButton {
              set_label: "no lyrics",
              set_hexpand: false,
              set_margin_end: 4,
              set_css_classes: &["pill", "caption"],
              inline_css: "padding: 0 0.75rem",
              connect_toggled[sender] => move |btn| {
                  sender.input(AppMsg::SetSearchFilter((TracksTableFilter::NoLyrics, btn.is_active())));
              },
            },

            gtk::ToggleButton {
              set_label: "not sync",
              set_hexpand: false,
              set_margin_end: 4,
              set_css_classes: &["pill", "caption"],
              inline_css: "padding: 0 0.75rem",
              connect_toggled[sender] => move |btn| {
                  sender.input(AppMsg::SetSearchFilter((TracksTableFilter::NotSync, btn.is_active())));
              },
            },

            gtk::ToggleButton {
              set_label: "never checked",
              set_hexpand: false,
              set_margin_end: 4,
              set_css_classes: &["pill", "caption"],
              inline_css: "padding: 0 0.75rem",
              connect_toggled[sender] => move |btn| {
                  sender.input(AppMsg::SetSearchFilter((TracksTableFilter::NeverChecked, btn.is_active())));
              },
            },

            gtk::ToggleButton {
              set_label: "not instrumental",
              set_hexpand: false,
              set_margin_end: 4,
              set_css_classes: &["pill", "caption"],
              inline_css: "padding: 0 0.75rem",
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
      set_default_size: (800, 800),
      set_title: Some(&app_title),

      #[local_ref]
      toast_overlay -> adw::ToastOverlay {
        adw::ToolbarView {
          add_top_bar: &header_bar,
          add_top_bar: &search_bar,

          gtk::Box {
            #[transition = "Crossfade"]
            match model.no_tracks {
              true => {
                gtk::Box {
                  set_align: gtk::Align::Center,

                  adw::StatusPage {
                    set_title: "No Tracks",
                    set_description: Some("Open Preferences to add a music library"),
                    set_icon_name: Some("edit-find-symbolic"),
                    set_width_request: 200,
                    #[wrap(Some)]
                    set_child = &gtk::Button {
                      set_label: "Add Library...",
                      set_css_classes: &["pill", "suggested-action"],
                      connect_clicked => AppMsg::ShowPrefsWindow,
                    },
                  },
                }
              }
              false => {
                adw::OverlaySplitView {
                  #[watch]
                  set_show_sidebar: model.is_sidebar_revealed,
                  #[watch]
                  set_collapsed: !model.is_sidebar_pinned,
                  set_sidebar_position: gtk::PackType::End,
                  set_enable_hide_gesture: true,
                  set_sidebar_width_fraction: 0.5,

                  // Tracks table view
                  #[wrap(Some)]
                  set_content = &gtk::ScrolledWindow {
                    set_hexpand: true,

                    #[local_ref]
                    tracks_table -> gtk::Overlay {}
                  },

                  // Sidebar
                  #[wrap(Some)]
                  set_sidebar = &gtk::ScrolledWindow {
                    set_hscrollbar_policy: gtk::PolicyType::Never,
                    add_css_class: "sidebar-pane",

                    #[name = "sidebar_viewport"]
                    gtk::Viewport {
                      #[watch]
                      set_child: Some(&model.sidebar_widget),
                    }
                  }
                }
              }
            },
          },
        },
      },
    }
  }

  menu! {
    main_menu: {
      "Fetch Lyrics" => ActionFetchLyrics,
      "Preferences" => ActionPrefs,
      section! {
        "About" => ActionAbout,
        },
      section! {
        "Debug" {
          "Test Toast" => ActionTestToast,
        },
      }
    }
  }

  fn init(
    init: Self::Init,
    root: Self::Root,
    sender: relm4::ComponentSender<Self>,
  ) -> relm4::ComponentParts<Self> {
    let app_title = if cfg!(debug_assertions) {
      format!("{APP_NAME_PRETTY} (DEBUG)")
    } else {
      APP_NAME_PRETTY.to_string()
    };

    let tracks_table_widget =
      TracksTableModel::builder()
        .launch(())
        .forward(sender.input_sender(), |msg| match msg {
          TracksTableOutput::RowActivated => AppMsg::ShowTrackDetailsSidebar,
          TracksTableOutput::TrackIdsSelected(set) => AppMsg::UpdateSelection(set),
        });
    let prefs_widget = PrefsModel::builder()
      .launch(())
      .forward(sender.input_sender(), |msg| match msg {
        PrefsOutput::RebuildTracksTable => AppMsg::BuildTracksTable,
      });

    let model = AppModel {
      libraries: vec![],
      tracks: vec![],
      tracks_table_widget,
      prefs_widget,
      about_widget: AboutModel::builder().launch(()).detach(),
      sidebar_widget: gtk::Box::new(gtk::Orientation::Vertical, 0),
      toaster: Toaster::default(),
      no_tracks: false,
      is_search_revealed: false,
      search_query: None,
      selection_state: SelectionState::None,
      last_selection_state: SelectionState::None,
      selected_track_id: None,
      selected_track_ids: HashSet::new(),
      is_sidebar_pinned: false,
      is_sidebar_revealed: false,
    };

    let toast_overlay = model.toaster.overlay_widget();
    let tracks_table = model.tracks_table_widget.widget();
    let widgets = view_output!();

    // Load libraries and tracks and populate table view
    sender.input(AppMsg::LoadLibraries);

    // Main menu actions
    relm4::new_action_group!(pub MainMenuActionGroup, "main_menu_action_group");
    let mut actions_group = RelmActionGroup::<MainMenuActionGroup>::new();

    relm4::new_stateless_action!(ActionFetchLyrics, MainMenuActionGroup, "fetch_lyrics");
    let action_fetch_lyrics: RelmAction<ActionFetchLyrics> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::FetchLyrics);
      })
    };
    actions_group.add_action(action_fetch_lyrics);

    relm4::new_stateless_action!(ActionPrefs, MainMenuActionGroup, "prefs");
    let action_prefs: RelmAction<ActionPrefs> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::ShowPrefsWindow);
      })
    };
    actions_group.add_action(action_prefs);

    relm4::new_stateless_action!(ActionAbout, MainMenuActionGroup, "about");
    let action_about: RelmAction<ActionAbout> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::ShowAbout);
      })
    };
    actions_group.add_action(action_about);

    let action_test_toast: RelmAction<ActionTestToast> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::ShowToast("Testing toast notification".into()));
      })
    };

    relm4::new_stateless_action!(ActionTestToast, MainMenuActionGroup, "test_toast");
    actions_group.add_action(action_test_toast);

    // Keyboard actions
    relm4::new_stateless_action!(ActionQuit, MainMenuActionGroup, "quit");
    let action_quit: RelmAction<ActionQuit> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::Quit);
      })
    };
    actions_group.add_action(action_quit);

    relm4::new_stateless_action!(ActionSearch, MainMenuActionGroup, "search");
    let action_search: RelmAction<ActionSearch> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender.input(AppMsg::ShowSearch(true));
      })
    };
    actions_group.add_action(action_search);

    // Keyboard shortcuts
    let app = relm4::main_adw_application();
    app.set_accelerators_for_action::<ActionPrefs>(&["<primary>comma"]);
    app.set_accelerators_for_action::<ActionQuit>(&["<primary>q"]);
    app.set_accelerators_for_action::<ActionSearch>(&["<primary>f"]);

    // Register menu/keyboard actions for main window
    actions_group.register_for_widget(&widgets.main_window);

    ComponentParts { model, widgets }
  }

  fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
    match message {
      AppMsg::AddLibrary(path) => todo!(),

      AppMsg::FetchLyrics => {
        self.tracks.iter_mut().for_each(|track| {
          let mut track = track.clone();
          sender.oneshot_command(async {
            let _ = track.fetch_lyrics().call().await;
            AppCommand::TrackUpdated(track)
          });
        });
      }

      AppMsg::ShowAbout => {
        debug!("Showing About window");
        let window = self.about_widget.widget();
        window.set_transient_for(Some(root));
        window.set_hide_on_close(true);
        window.present();
      }

      AppMsg::ShowPrefsWindow => {
        debug!("Showing Preferences window");
        let window = self.prefs_widget.widget();
        window.set_transient_for(Some(root));
        window.set_hide_on_close(true);
        window.present();
      }

      // TODO: Use alert dialog to show errors
      AppMsg::LoadLibraries => {
        if self
          .load_libraries()
          .inspect_err(|e| {
            sender.input(AppMsg::ShowToast(format!(
              "Error loading music libraries: {e}"
            )))
          })
          .is_ok()
        {
          // Update the table view
          sender.input(AppMsg::BuildTracksTable);

          self.no_tracks = self.tracks.is_empty();

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

      AppMsg::BuildTracksTable => self
        .tracks_table_widget
        .sender()
        .emit(TracksTableMsg::ClearAndAppend(self.tracks.clone())),

      AppMsg::RefreshLibraries => todo!(),

      AppMsg::SearchQueryChanged(query) => {
        debug!("Searching for: {}", &query);
        self.is_search_revealed = true;
        self.search_query = if !query.is_empty() { Some(query) } else { None };

        self
          .tracks_table_widget
          .sender()
          .emit(TracksTableMsg::Filter(self.search_query.clone()));
      }

      AppMsg::ShowToast(msg) => {
        debug!("Emit toast notification: \"{}\"", &msg);
        let toast = adw::Toast::builder().title(msg).timeout(5).build();
        self.toaster.add_toast(toast);
      }

      AppMsg::ShowSearch(active) => {
        if active {
          debug!("Search bar revealed");
        } else {
          debug!("Search bar hidden");
          self.search_query = None;
        }

        self.is_search_revealed = active;
      }

      AppMsg::SetSearchFilter((filter, active)) => {
        debug!("Search filter \"{:?}\" active: {}", &filter, active);
        self
          .tracks_table_widget
          .sender()
          .emit(TracksTableMsg::SetFilter((filter, active)));
      }

      AppMsg::ShowTrackDetailsSidebar => {
        debug!("Showing sidebar");
        self.is_sidebar_revealed = true;
        self.rebuild_sidebar_widget();
      }

      AppMsg::HideTrackDetailsSidebar => {
        debug!("Hiding sidebar");
        self.is_sidebar_revealed = false;
        self.is_sidebar_pinned = false;
      }

      AppMsg::PinTrackDetailsSidebar(active) => {
        debug!("Pinning sidebar: {active}");
        self.is_sidebar_pinned = active;
        self.is_sidebar_revealed = active;
        if active {
          self.rebuild_sidebar_widget();
        };
      }

      AppMsg::UpdateSelection(set) => {
        self.selected_track_ids = set;
        debug!("Tracks selected: {}", self.selected_track_ids.len());
        trace!("Selected Track IDs:\n{:#?}", self.selected_track_ids);

        // Selection changed; hide track details unless pinned
        if !self.is_sidebar_pinned {
          self.is_sidebar_revealed = false;
        }

        if self.selected_track_ids.len() == 1 {
          self.selected_track_id = self.selected_track_ids.iter().next().copied();
          self.change_selection_state(SelectionState::Single);
        } else if self.selected_track_ids.len() > 1 {
          self.selected_track_id = None;
          self.change_selection_state(SelectionState::Multi);
        } else {
          self.selected_track_id = None;
          self.change_selection_state(SelectionState::None);
        }
      }

      AppMsg::Quit => {
        let app = relm4::main_application();
        app.quit();
      }
    }
  }

  fn update_cmd(
    &mut self,
    message: Self::CommandOutput,
    sender: ComponentSender<Self>,
    root: &Self::Root,
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
          .emit(TracksTableMsg::Update(track));
      }
    }
  }
}

impl AppModel {
  fn add_libraries(&mut self) -> Result<()> {
    debug!("Loading Libraries and Tracks ...");

    self.libraries = Library::get_all()?;
    self.load_tracks()?;

    debug!("Loaded {} Libraries", self.libraries.len());

    Ok(())
  }

  fn load_libraries(&mut self) -> Result<()> {
    debug!("Loading Libraries and Tracks ...");

    self.libraries = Library::get_all()?;
    self.load_tracks()?;

    debug!("Loaded {} Libraries", self.libraries.len());

    Ok(())
  }

  fn refresh_libraries(&mut self) -> Result<()> {
    debug!("Refreshing Libraries ...");

    self.libraries = Library::get_all()?;
    for lib in &self.libraries {
      lib.refresh().call()?;
    }
    self.load_tracks()?;

    debug!("Refreshed {} Libraries", self.libraries.len());

    Ok(())
  }

  fn load_tracks(&mut self) -> Result<()> {
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

    debug!(
      "Loaded {} Tracks from {} Libraries",
      self.tracks.len(),
      self.libraries.len()
    );

    Ok(())
  }

  fn rebuild_sidebar_widget(&mut self) {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 24);
    root.set_margin_all(12);
    if let Some(track) = self
      .selected_track_id
      .and_then(|id| self.tracks.iter().find(|t| t.id == id))
    {
      debug!("Building sidebar with {track} selected");

      root.set_halign(gtk::Align::Fill);

      // General track info
      let pg = adw::PreferencesGroup::new();
      pg.set_title("Track Details");
      let ar = adw::ActionRow::new();
      ar.set_title("Artist Name");
      ar.set_subtitle(&track.artist_name);
      pg.container_add(&ar);
      let ar = adw::ActionRow::new();
      ar.set_title("Album Title");
      ar.set_subtitle(&track.album_name);
      pg.container_add(&ar);
      let ar = adw::ActionRow::new();
      ar.set_title("Track Title");
      ar.set_subtitle(&track.track_name);
      pg.container_add(&ar);
      let ar = adw::ActionRow::new();
      ar.set_title("Duration");
      ar.set_subtitle(&format!(
        "{}:{:02}",
        track.duration as u32 / 60,
        track.duration as u32 % 60,
      ));
      pg.container_add(&ar);
      let ar = adw::ActionRow::new();
      ar.set_title("File Date");
      ar.set_subtitle(&util::ndt_utc_to_ui_string(track.file_modified_at));
      pg.container_add(&ar);
      root.append(&pg);

      // Lyrics info
      let inner = gtk::Box::new(gtk::Orientation::Vertical, 12);

      let pg = adw::PreferencesGroup::new();
      pg.set_title("Lyrics");
      let ar = adw::ActionRow::new();
      ar.set_title("Lyrics Tag");
      ar.set_subtitle(if track.lyrics.is_some() {
        "Present"
      } else {
        "Missing"
      });
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

      if track.lyrics_sidecar_lrc_file.is_some() || track.lyrics_sidecar_txt_file.is_some() {
        let mut sidecar_formats = vec![];
        if track.lyrics_sidecar_lrc_file.is_some() {
          sidecar_formats.push("LRC");
        }
        if track.lyrics_sidecar_txt_file.is_some() {
          sidecar_formats.push("TXT");
        }

        let pg = adw::PreferencesGroup::new();
        let ar = adw::ActionRow::new();
        ar.set_title("Sidecar File");
        ar.set_subtitle(&sidecar_formats.join(", "));
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
          .map(|ndt| util::ndt_utc_to_ui_string(ndt))
          .unwrap_or("Never".into()),
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
        ar.set_subtitle(&&util::ndt_utc_to_ui_string(track.added_at));
        pg.container_add(&ar);
        let ar = adw::ActionRow::new();
        ar.set_title("Updated At");
        ar.set_subtitle(&util::ndt_utc_to_ui_string(track.updated_at));
        pg.container_add(&ar);
        let ar = adw::ActionRow::new();
        ar.set_title("Refreshed At");
        ar.set_subtitle(&&util::ndt_utc_to_ui_string(track.refreshed_at));
        pg.container_add(&ar);
        root.append(&pg);
      }
    } else if self.selected_track_ids.len() > 1 {
      debug!("Building sidebar with multiple tracks selected");

      root.set_valign(gtk::Align::Center);
      let selected = self.selected_track_ids.len();
      let label = gtk::Label::new(Some(&format!("{selected} tracks selected")));
      root.append(&label);
    } else {
      debug!("Building sidebar with no track selected");

      root.set_valign(gtk::Align::Center);
      let label = gtk::Label::new(Some("No track selected"));
      root.append(&label);
    }

    self.sidebar_widget = root;
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
