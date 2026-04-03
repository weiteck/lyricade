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
use crate::ui::tracks_table::{TracksTableModel, TracksTableMsg};
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
    FetchLyrics,
    Quit,
    ReloadLibraries,
    ShowAbout,
    SearchQueryChanged(String),
    ShowSearch(bool),
    ShowSettings,
    ShowToast(String),
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
        set_key_capture_widget: Some(&root),

        #[wrap(Some)]
        set_child = &gtk::SearchEntry {
          set_hexpand: true,
          set_placeholder_text: Some("Type to search"),

          connect_search_changed[sender] => move |query| {
            sender.input(AppMsg::SearchQueryChanged(query.text().to_string()))
          },

          connect_stop_search => AppMsg::ShowSearch(false),
        },
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
                      tracks_table -> gtk::Box {}
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
        sender.input(AppMsg::ReloadLibraries);

        // Setup main menu
        relm4::new_action_group!(pub MainMenuActionGroup, "main_menu_action_group");
        relm4::new_stateless_action!(ActionFetchLyrics, MainMenuActionGroup, "fetch_lyrics");
        relm4::new_stateless_action!(ActionSettings, MainMenuActionGroup, "settings");
        relm4::new_stateless_action!(ActionAbout, MainMenuActionGroup, "about");
        relm4::new_stateless_action!(ActionTestToast, MainMenuActionGroup, "test_toast");

        // Keyboard actions
        relm4::new_stateless_action!(ActionQuit, MainMenuActionGroup, "quit");
        relm4::new_stateless_action!(ActionSearch, MainMenuActionGroup, "search");

        let action_fetch_lyrics: RelmAction<ActionFetchLyrics> = {
            let sender = sender.clone();
            RelmAction::new_stateless(move |_| {
                sender.input(AppMsg::FetchLyrics);
            })
        };
        let action_settings: RelmAction<ActionSettings> = {
            let sender = sender.clone();
            RelmAction::new_stateless(move |_| {
                sender.input(AppMsg::ShowSettings);
            })
        };
        let action_about: RelmAction<ActionAbout> = {
            let sender = sender.clone();
            RelmAction::new_stateless(move |_| {
                sender.input(AppMsg::ShowAbout);
            })
        };
        let action_test_toast: RelmAction<ActionTestToast> = {
            let sender = sender.clone();
            RelmAction::new_stateless(move |_| {
                sender.input(AppMsg::ShowToast("Testing toast notification".into()));
            })
        };

        // Actions for keyboard
        let action_quit: RelmAction<ActionQuit> = {
            let sender = sender.clone();
            RelmAction::new_stateless(move |_| {
                sender.input(AppMsg::Quit);
            })
        };
        let action_search: RelmAction<ActionSearch> = {
            let sender = sender.clone();
            RelmAction::new_stateless(move |_| {
                sender.input(AppMsg::ShowSearch(true));
            })
        };

        let mut actions_group = RelmActionGroup::<MainMenuActionGroup>::new();

        actions_group.add_action(action_fetch_lyrics);
        actions_group.add_action(action_settings);
        actions_group.add_action(action_about);
        actions_group.add_action(action_test_toast);
        actions_group.add_action(action_quit);
        actions_group.add_action(action_search);

        actions_group.register_for_widget(&widgets.main_window);

        let app = relm4::main_adw_application();
        app.set_accelerators_for_action::<ActionSettings>(&["<primary>,"]);
        app.set_accelerators_for_action::<ActionQuit>(&["<primary>Q"]);
        app.set_accelerators_for_action::<ActionSearch>(&["<primary>F"]);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match message {
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
            AppMsg::ReloadLibraries => {
                if self
                    .reload_libraries()
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

            AppMsg::ShowSearch(toggle) => {
                if toggle {
                    debug!("Search bar revealed");
                } else {
                    debug!("Search bar hidden");
                    self.search_query = None;
                }

                self.is_search_revealed = toggle;
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
    pub fn reload_libraries(&mut self) -> Result<()> {
        debug!("Reloading Libraries and Tracks ...");

        self.libraries = Library::get_all()?;

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
            "Reloaded {} Libraries with {} Tracks",
            self.libraries.len(),
            self.tracks.len()
        );

        Ok(())
    }
}

pub fn start() -> Result<()> {
    let app = RelmApp::new("io.github.weiteck.lrc-lyrics");
    Ok(app.run::<AppModel>(()))
}
