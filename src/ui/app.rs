use camino::Utf8PathBuf;
use relm4::abstractions::Toaster;
use relm4::actions::AccelsPlus;
use relm4::actions::{RelmAction, RelmActionGroup};
use relm4::adw::prelude::*;
use relm4::*;
use tracing::{debug, error};

use crate::SETTINGS;
use crate::settings::APP_NAME_PRETTY;
use crate::ui::about::AboutModel;
use crate::ui::settings::SettingsModel;
use crate::ui::tracks_table::{TracksTableFilter, TracksTableModel, TracksTableMsg};
use crate::{Result, library::Library, track::Track};

struct AppModel {
    libraries: Vec<Library>,
    tracks: Vec<Track>,

    tracks_table_widget: Controller<TracksTableModel>,
    settings_widget: Controller<SettingsModel>,
    about_widget: Controller<AboutModel>,
    toaster: Toaster,

    is_empty: bool,

    is_search_revealed: bool,
    search_query: Option<String>,
}

#[derive(Debug)]
enum AppMsg {
    AddLibrary(Utf8PathBuf),
    FetchLyrics,
    Quit,
    /// Load libraries and tracks from the database.
    LoadLibraries,
    /// Scan library paths for changes.
    RefreshLibraries,
    ShowAbout,
    SearchQueryChanged(String),
    ShowSearch(bool),
    ShowSettings,
    ShowToast(String),
    SetSearchFilter((TracksTableFilter, bool)),
}

#[derive(Debug)]
enum AppCommand {
    TrackUpdated(Track),
}

#[relm4::component]
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
        set_title: Some(APP_NAME_PRETTY),

        #[local_ref]
        toast_overlay -> adw::ToastOverlay {
          adw::ToolbarView {
            add_top_bar: &header_bar,
            add_top_bar: &search_bar,

            gtk::Box {
              set_orientation: gtk::Orientation::Vertical,
              set_margin_all: 5,

              #[transition = "Crossfade"]
              match model.is_empty {
                  true => {
                    gtk::Box {
                      set_align: gtk::Align::Center,

                      adw::StatusPage {
                        set_title: "No Tracks",
                        set_description: Some("Open Settings to add a music library"),
                        set_icon_name: Some("edit-find-symbolic"),
                        set_width_request: 200,
                        #[wrap(Some)]
                        set_child = &gtk::Button {
                          set_label: "Add Library",
                          set_css_classes: &["pill", "suggested-action"],
                          connect_clicked => AppMsg::ShowSettings,
                        },
                      },
                    }
                  }
                  false => {
                    gtk::ScrolledWindow {
                      set_hexpand: true,

                      #[local_ref]
                      tracks_table -> gtk::Overlay {}
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
        "Settings" => ActionSettings,
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
        let model = AppModel {
            libraries: vec![],
            tracks: vec![],
            tracks_table_widget: TracksTableModel::builder().launch(()).detach(),
            settings_widget: SettingsModel::builder().launch(()).detach(),
            about_widget: AboutModel::builder().launch(()).detach(),
            toaster: Toaster::default(),
            is_empty: false,
            is_search_revealed: false,
            search_query: None,
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

        relm4::new_stateless_action!(ActionSettings, MainMenuActionGroup, "settings");
        let action_settings: RelmAction<ActionSettings> = {
            let sender = sender.clone();
            RelmAction::new_stateless(move |_| {
                sender.input(AppMsg::ShowSettings);
            })
        };
        actions_group.add_action(action_settings);

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
        app.set_accelerators_for_action::<ActionSettings>(&["<primary>comma"]);
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
                        let _ = track
                            .fetch_lyrics()
                            .options(SETTINGS.fetch_lyrics)
                            .call()
                            .await;
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

            AppMsg::ShowSettings => {
                debug!("Showing Settings window");
                let window = self.settings_widget.widget();
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
                    self.tracks_table_widget.sender().emit(
                        super::tracks_table::TracksTableMsg::ClearAndAppend(self.tracks.clone()),
                    );

                    self.is_empty = self.tracks.is_empty();

                    sender.input(AppMsg::ShowToast(format!(
                        "Loaded {} music libraries with {} tracks",
                        self.libraries.len(),
                        self.tracks.len()
                    )));
                }
            }

            AppMsg::RefreshLibraries => todo!(),

            AppMsg::SearchQueryChanged(query) => {
                debug!("Searching for: {}", &query);
                self.is_search_revealed = true;
                self.search_query = if !query.is_empty() { Some(query) } else { None };

                self.tracks_table_widget
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
                self.tracks_table_widget
                    .sender()
                    .emit(TracksTableMsg::SetFilter((filter, active)));
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
                self.tracks_table_widget
                    .sender()
                    .emit(TracksTableMsg::Update(track));
            }
        }
    }
}

impl AppModel {
    pub fn add_libraries(&mut self) -> Result<()> {
        debug!("Loading Libraries and Tracks ...");

        self.libraries = Library::get_all()?;
        self.load_tracks()?;

        debug!("Loaded {} Libraries", self.libraries.len());

        Ok(())
    }

    pub fn load_libraries(&mut self) -> Result<()> {
        debug!("Loading Libraries and Tracks ...");

        self.libraries = Library::get_all()?;
        self.load_tracks()?;

        debug!("Loaded {} Libraries", self.libraries.len());

        Ok(())
    }

    pub fn refresh_libraries(&mut self) -> Result<()> {
        debug!("Refreshing Libraries ...");

        self.libraries = Library::get_all()?;
        for lib in &self.libraries {
            lib.refresh().call()?;
        }
        self.load_tracks()?;

        debug!("Refreshed {} Libraries", self.libraries.len());

        Ok(())
    }

    pub fn load_tracks(&mut self) -> Result<()> {
        debug!("Loading Tracks from {} Libraries ...", self.libraries.len());

        self.tracks = self
            .libraries
            .iter()
            .filter_map(|lib| {
                lib.tracks()
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
}

pub fn start() -> Result<()> {
    let app = RelmApp::new("io.github.weiteck.lrc-lyrics");
    Ok(app.run::<AppModel>(()))
}
