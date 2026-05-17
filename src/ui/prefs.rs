#![expect(clippy::bool_to_int_with_if)]

use std::{collections::HashSet, path::PathBuf};

use adw::prelude::*;
use camino::Utf8PathBuf;
use relm4::{
  adw::PreferencesWindow,
  gtk::{EventControllerKey, gdk, glib},
  prelude::*,
};
use relm4_components::open_dialog::{
  OpenDialog, OpenDialogMsg, OpenDialogResponse, OpenDialogSettings,
};
use tracing::{debug, error, trace};

use crate::{
  SETTINGS,
  library::Library,
  lyrics::LyricsType,
  settings::Settings,
  ui::prefs::library_row::{LibraryRow, LibraryRowMsg, LibraryRowOutput},
  util::{self, now},
};

mod library_row;

pub struct PrefsModel {
  root: PreferencesWindow,

  libraries: HashSet<Library>,
  library_rows: FactoryVecDeque<LibraryRow>,

  editing_library_row: Option<DynamicIndex>,

  settings_initial: Settings,
  settings_current: Settings,
  settings_default: Settings,

  add_library_file_dialog: Controller<OpenDialog>,
  edit_library_file_dialog: Controller<OpenDialog>,
}

#[derive(Debug)]
pub enum PrefsMsg {
  DefaultSettings,
  RevertSettings,
  SaveSettings,
  UpdateSetting(ExposedSetting),

  AddLibraryFileDialogRequest,
  AddLibraryFileDialogResponse(PathBuf),

  EditLibraryFileDialogRequest(DynamicIndex),
  EditLibraryFileDialogResponse(PathBuf),

  DeleteLibraryRow(DynamicIndex),

  ShowToast(String, bool),

  CloseRequested,

  NoOp,
}

#[derive(Debug)]
pub enum PrefsOutput {
  Close, // request parent window to close Prefs window
}

#[derive(Debug)]
pub enum ExposedSetting {
  PreferLyricsType(LyricsType),
  PreferIsoTimestamps(bool),

  ScanNewFilesOnly(bool),
  UpgradeLyricsTagOnScan(bool),
  HandleSidecar(HandleSidecarSetting),

  UpdateLyricsTagOnFetch(bool),
  SaveSidecarOnFetch(bool),
}

#[derive(Debug)]
pub enum HandleSidecarSetting {
  DoNothing,
  KeepOne,
  Delete,
}

#[relm4::component(pub)]
impl SimpleComponent for PrefsModel {
  type Input = PrefsMsg;
  type Output = PrefsOutput;
  type Init = (Settings, Vec<Library>);

  view! {
    prefs_window = adw::PreferencesWindow {
      set_title: Some("Preferences"),
      set_search_enabled: false,
      set_size_request: (500, 300),
      set_default_size: (600, 600),

      // Update and save settings on close
      connect_close_request[sender] => move |_| {
        sender.input(PrefsMsg::SaveSettings);
        glib::Propagation::Proceed
      },

      add = &adw::PreferencesPage {
        set_title: "_General",
        set_use_underline: true,
        set_icon_name: Some("preferences-system-symbolic"),

        adw::PreferencesGroup {
          set_title: "Preferred Lyrics Format",
          set_description: Some("Choose what lyrics format to prefer when fetching lyrics online or cleaning up multiple sidecar files."),

          gtk::ListBox {
            add_css_class: "boxed-list",

            adw::ActionRow {
              set_title: "_Synchronous",
              set_use_underline: true,
              set_subtitle: "Prefer LRC format lyrics",
              set_selectable: false,

              set_activatable_widget: Some(&group_lyrics_type_button_sync),
              #[name = "group_lyrics_type_button_sync"]
              add_prefix = &gtk::CheckButton {
                #[watch]
                set_active: model.settings_current.prefer_lyrics_type == LyricsType::Sync,
                connect_toggled[sender] => move |btn| {
                  if btn.is_active() {
                    sender.input(PrefsMsg::UpdateSetting(ExposedSetting::PreferLyricsType(LyricsType::Sync)));
                  }
                },
              },
            },

            adw::ActionRow {
              set_title: "_Plain",
              set_use_underline: true,
              set_subtitle: "Prefer TXT format lyrics",
              set_selectable: false,

              set_activatable_widget: Some(&group_lyrics_type_button_plain),
              #[name = "group_lyrics_type_button_plain"]
              add_prefix = &gtk::CheckButton {
                set_group: Some(&group_lyrics_type_button_sync),
                #[watch]
                set_active: model.settings_current.prefer_lyrics_type == LyricsType::Plain,
                connect_toggled[sender] => move |btn| {
                  if btn.is_active() {
                    sender.input(PrefsMsg::UpdateSetting(ExposedSetting::PreferLyricsType(LyricsType::Plain)));
                  }
                },
              },
            },
          },
        },

        adw::PreferencesGroup {
          set_title: "Scan Files",
          set_description: Some("How audio files are scanned."),

          adw::SwitchRow {
            set_title: "_Ignore Unchanged",
            set_use_underline: true,
            set_subtitle: "Only scan new or modified files",
            #[watch]
            set_active: model.settings_current.scan_new_files_only,
            connect_active_notify[sender] => move |btn| {
              sender.input(PrefsMsg::UpdateSetting(ExposedSetting::ScanNewFilesOnly(btn.is_active())));
            }
          },
        },

        adw::PreferencesGroup {
          set_title: "Fetch Lyrics",
          set_description: Some("Choose what to do with the lyrics sourced from <i>lrclib.net</i>"),

          adw::SwitchRow {
            set_title: "Write to Lyrics _Tag",
            set_use_underline: true,
            set_subtitle: "Update audio file metadata",
            #[watch]
            set_active: model.settings_current.update_lyrics_tag_on_fetch,
            connect_active_notify[sender] => move |btn| {
              sender.input(PrefsMsg::UpdateSetting(ExposedSetting::UpdateLyricsTagOnFetch(btn.is_active())));
            }
          },

          adw::SwitchRow {
            set_title: "Write to Sidecar _File",
            set_use_underline: true,
            set_subtitle: "Save LRC/TXT lyrics files alongside audio files",
            #[watch]
            set_active: model.settings_current.save_sidecar_file_on_fetch,
            connect_active_notify[sender] => move |btn| {
              sender.input(PrefsMsg::UpdateSetting(ExposedSetting::SaveSidecarOnFetch(btn.is_active())));
            },

          },
        },

        adw::PreferencesGroup {
          set_title: "Sidecar Files",
          set_description: Some("How LRC/TXT lyrics files are scanned and managed."),

          adw::SwitchRow {
            set_title: "_Upgrade Lyrics Tag From File",
            set_use_underline: true,
            set_subtitle: "Upgrade lyrics tags to the preferred format if a sidecar file of that format is found",
            #[watch]
            set_active: model.settings_current.upgrade_lyrics_tag_on_scan,
            connect_active_notify[sender] => move |btn| {
              sender.input(PrefsMsg::UpdateSetting(ExposedSetting::UpgradeLyricsTagOnScan(btn.is_active())));
            }
          },

          adw::ComboRow {
            set_title: "_Clean Up Sidecar Files",
            set_use_underline: true,
            #[watch]
            set_subtitle: &format!("Whether lyrics files are deleted\n{}", if model.settings_current.delete_sidecar_files_on_scan {
              "Action: All sibling files with the same name as an audio file but with a “.lrc” or “.txt” extension will be deleted"
            } else if model.settings_current.keep_one_sidecar_file_on_scan  {
              "Action: Keep only the preferred lyrics format if both sync and plain sidecar files are present"
            } else {
              "Action: Keep all sidecar files"
            }),
            set_model: Some(&gtk::StringList::new(&[
              "Do Nothing",
              "Keep One",
              "Delete",
            ])),
            #[watch]
            set_selected: if model.settings_current.delete_sidecar_files_on_scan { 2 }
              else if model.settings_current.keep_one_sidecar_file_on_scan { 1 }
              else { 0 },
            connect_selected_item_notify[sender] => move |row| {
              match row.selected() {
                1 => {
                  sender.input(PrefsMsg::UpdateSetting(ExposedSetting::HandleSidecar(HandleSidecarSetting::KeepOne)));
                }
                2 => {
                  sender.input(PrefsMsg::UpdateSetting(ExposedSetting::HandleSidecar(HandleSidecarSetting::Delete)));
                }
                _ => {
                  sender.input(PrefsMsg::UpdateSetting(ExposedSetting::HandleSidecar(HandleSidecarSetting::DoNothing)));
                }
              }
            },
          },
        },

        adw::PreferencesGroup {
          set_title: "Date and Time Format",
          set_description: Some("Choose how dates and times and displayed in the track list view."),

          gtk::ListBox {
            add_css_class: "boxed-list",

            adw::ActionRow {
              set_title: "Simpl_e",
              set_use_underline: true,
              set_subtitle: &format!("Examples: “{}”, “{}”", example_datetime_simple1, example_datetime_simple2),
              set_selectable: false,

              set_activatable_widget: Some(&group_datetime_format_button_simple),
              #[name = "group_datetime_format_button_simple"]
              add_prefix = &gtk::CheckButton {
                #[watch]
                set_active: !model.settings_current.prefer_accurate_timestamps,
                connect_toggled[sender] => move |btn| {
                  sender.input(PrefsMsg::UpdateSetting(ExposedSetting::PreferIsoTimestamps(!btn.is_active())));
                },
              },
            },

            adw::ActionRow {
              set_title: "_Accurate",
              set_use_underline: true,
              set_subtitle: &format!("Examples: “{}”, “{}”", example_datetime_accurate1, example_datetime_accurate2),
              set_selectable: false,

              set_activatable_widget: Some(&group_datetime_format_button_accurate),
              #[name = "group_datetime_format_button_accurate"]
              add_prefix = &gtk::CheckButton {
                set_group: Some(&group_datetime_format_button_simple),
                #[watch]
                set_active: model.settings_current.prefer_accurate_timestamps,
                connect_toggled[sender] => move |btn| {
                  sender.input(PrefsMsg::UpdateSetting(ExposedSetting::PreferIsoTimestamps(btn.is_active())));
                },
              },
            },
          },
        },

        adw::PreferencesGroup {
          gtk::Box {
            set_orientation: gtk::Orientation::Horizontal,
            set_halign: gtk::Align::Center,
            set_spacing: 12,

              gtk::Button {
                set_label: "Revert Changes",
                #[watch]
                set_sensitive: model.settings_current != model.settings_initial,
                connect_clicked => PrefsMsg::RevertSettings,
              },
              gtk::Button {
                set_label: "Use Defaults",
                #[watch]
                set_sensitive: model.settings_current != model.settings_default,
                connect_clicked => PrefsMsg::DefaultSettings,
              },
          },
        },
      },

      add = &adw::PreferencesPage {
        set_name: Some("libraries"),
        #[watch]
        set_title: &format!("_Music Libraries ({})", model.libraries.len()),
        set_use_underline: true,
        set_visible: true,
        set_icon_name: Some("folder-music-symbolic"),

        adw::PreferencesGroup {
          set_title: "Music Libraries",
          set_description: Some("Add, remove or edit Music Libraries. A Music Library is a path to search for audio files."),

          #[local_ref]
          libraries_list_box -> gtk::ListBox {
            set_selection_mode: gtk::SelectionMode::None,
            add_css_class: "boxed-list",

            // Add library button
            adw::ActionRow {
              set_halign: gtk::Align::Fill,
              set_hexpand: true,
              set_activatable: true,
              add_css_class: "button",
              set_activatable_widget: Some(&add_row_widget),
              connect_activated => PrefsMsg::AddLibraryFileDialogRequest,

              #[wrap(Some)]
              #[name = "add_row_widget"]
              set_child = &gtk::Box {
                set_align: gtk::Align::Center,
                set_margin_all: 2,
                set_spacing: 4,

                gtk::Image {
                  set_icon_name: Some("list-add-symbolic"),
                },
                gtk::Label {
                  set_label: "_Add Music Library",
                  set_use_underline: true,
                  add_css_class: "title",
                }
              },
            },
          },
        },
      },
    },
  }

  fn init(
    (settings, libraries): Self::Init,
    root: Self::Root,
    sender: ComponentSender<Self>,
  ) -> ComponentParts<Self> {
    // Recent datetime to use as example in UI
    let ndt = now()
      .checked_sub_days(chrono::Days::new(3))
      .expect("should be a valid date");
    let example_datetime_simple1 = util::ndt_utc_to_humanised_string(ndt);
    let example_datetime_accurate1 = util::ndt_utc_to_ui_string(ndt);
    let ndt = ndt
      .checked_sub_days(chrono::Days::new(50))
      .expect("should be a valid date");
    let example_datetime_simple2 = util::ndt_utc_to_humanised_string(ndt);
    let example_datetime_accurate2 = util::ndt_utc_to_ui_string(ndt);

    let libraries = libraries.into_iter().collect::<HashSet<_>>();

    // Build library rows
    let library_rows = build_library_rows(libraries.clone(), &sender);

    // Create file dialogs
    let file_dialog_settings = OpenDialogSettings {
      folder_mode: true,
      accept_label: "Use Folder".into(),
      create_folders: false,
      is_modal: true,
      ..Default::default()
    };
    let add_library_file_dialog = OpenDialog::builder()
      .transient_for_native(&root)
      .launch(file_dialog_settings.clone())
      .forward(sender.input_sender(), |resp| match resp {
        OpenDialogResponse::Accept(path) => PrefsMsg::AddLibraryFileDialogResponse(path),
        OpenDialogResponse::Cancel => PrefsMsg::NoOp,
      });
    let edit_library_file_dialog = OpenDialog::builder()
      .transient_for_native(&root)
      .launch(file_dialog_settings)
      .forward(sender.input_sender(), |resp| match resp {
        OpenDialogResponse::Accept(path) => PrefsMsg::EditLibraryFileDialogResponse(path),
        OpenDialogResponse::Cancel => PrefsMsg::NoOp,
      });

    let model = PrefsModel {
      libraries,
      library_rows,
      editing_library_row: None,
      settings_initial: settings.clone(),
      settings_current: settings,
      settings_default: Settings::default(),
      add_library_file_dialog,
      edit_library_file_dialog,
      root: root.clone(),
    };

    let libraries_list_box = model.library_rows.widget();
    let widgets = view_output!();

    // Esc, Ctrl-W key presses close the window
    let sender_handle = sender.clone();
    let controller = EventControllerKey::new();
    controller.connect_key_pressed(move |_con, key, _idx, modifier| {
      trace!("Prefs key event: key {key} + {:?}", modifier);
      if key == gdk::Key::Escape
        || (key.to_upper() == gdk::Key::W && modifier.contains(gdk::ModifierType::CONTROL_MASK))
      {
        sender_handle.input(PrefsMsg::CloseRequested);
      }
      glib::Propagation::Proceed
    });
    root.add_controller(controller);

    // Start at Music Libraries page if empty
    if model.libraries.is_empty() {
      model.root.set_visible_page_name("libraries");
    }

    ComponentParts { model, widgets }
  }

  fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
    match message {
      PrefsMsg::DefaultSettings => {
        debug!("Setting default settings");
        self.settings_current = self.settings_default.clone();
      }

      PrefsMsg::RevertSettings => {
        debug!("Restoring changed settings");
        self.settings_current = self.settings_initial.clone();
      }

      PrefsMsg::SaveSettings => {
        if self.settings_current != self.settings_initial {
          if let Ok(mut guard) = SETTINGS.write() {
            *guard = self.settings_current.clone();
            let _ = guard.save();
          } else {
            error!("Settings lock was poisoned while closing Preferences");
          }
        }

        sender
          .output(PrefsOutput::Close)
          .expect("PrefsOutput receiver dropped");
      }

      PrefsMsg::UpdateSetting(setting) => match setting {
        ExposedSetting::PreferLyricsType(lyrics_type) => {
          debug!("UpdateSetting: PreferLyricsType: {lyrics_type}");
          self.settings_current.prefer_lyrics_type = lyrics_type;
        }
        ExposedSetting::PreferIsoTimestamps(active) => {
          debug!("UpdateSetting: PreferIsoTimestamps: {active}");
          self.settings_current.prefer_accurate_timestamps = active;
        }
        ExposedSetting::ScanNewFilesOnly(active) => {
          debug!("UpdateSetting: ScanNewFilesOnly: {active}");
          self.settings_current.scan_new_files_only = active;
        }
        ExposedSetting::UpgradeLyricsTagOnScan(active) => {
          debug!("UpdateSetting: UpgradeLyricsTagOnScan: {active}");
          self.settings_current.upgrade_lyrics_tag_on_scan = active;
        }
        ExposedSetting::HandleSidecar(setting) => {
          debug!("UpdateSetting: ManageSidecarFiles: {setting:?}");
          match setting {
            HandleSidecarSetting::DoNothing => {
              self.settings_current.delete_sidecar_files_on_scan = false;
              self.settings_current.keep_one_sidecar_file_on_scan = false;
            }
            HandleSidecarSetting::KeepOne => {
              self.settings_current.delete_sidecar_files_on_scan = false;
              self.settings_current.keep_one_sidecar_file_on_scan = true;
            }
            HandleSidecarSetting::Delete => {
              self.settings_current.delete_sidecar_files_on_scan = true;
              self.settings_current.keep_one_sidecar_file_on_scan = false;
            }
          }
        }
        ExposedSetting::UpdateLyricsTagOnFetch(active) => {
          debug!("UpdateSetting: UpdateLyricsTagOnFetch: {active}");
          self.settings_current.update_lyrics_tag_on_fetch = active;
          if !active {
            self.settings_current.save_sidecar_file_on_fetch = true;
          }
        }
        ExposedSetting::SaveSidecarOnFetch(active) => {
          debug!("UpdateSetting: SaveSidecarFileOnFetch: {active}");
          self.settings_current.save_sidecar_file_on_fetch = active;
          if !active {
            self.settings_current.update_lyrics_tag_on_fetch = true;
          }
        }
      },

      PrefsMsg::AddLibraryFileDialogRequest => {
        debug!("Opening add library file dialog");

        self.add_library_file_dialog.emit(OpenDialogMsg::Open);
      }

      PrefsMsg::AddLibraryFileDialogResponse(path) => {
        debug!("Adding library path: {}", path.to_string_lossy());

        if let Ok(path) = Utf8PathBuf::from_path_buf(path) {
          match Library::add(&path) {
            Ok(lib) => {
              self.library_rows.guard().push_back(lib.clone());
              self.libraries.insert(lib);

              sender.input(PrefsMsg::ShowToast("Library added".into(), false));
            }
            Err(error) => sender.input(PrefsMsg::ShowToast(error.to_string(), true)),
          }
        }
      }

      PrefsMsg::EditLibraryFileDialogRequest(idx) => {
        debug!("Opening edit library file dialog");

        self.editing_library_row = Some(idx);
        self.edit_library_file_dialog.emit(OpenDialogMsg::Open);
      }

      PrefsMsg::EditLibraryFileDialogResponse(path) => {
        debug!("Selected library path: {}", path.to_string_lossy());

        if let Some(idx) = &self.editing_library_row
          && let Some(lib_row) = self
            .library_rows
            .iter()
            .find(|&lr| lr.index.current_index() == idx.current_index())
          && let Ok(path) = Utf8PathBuf::from_path_buf(path)
        {
          lib_row.sender.input(LibraryRowMsg::UpdatePath(path));
        }

        self.editing_library_row = None;
      }

      // TODO: Show toast with 'undo' button
      PrefsMsg::DeleteLibraryRow(idx) => {
        debug!("DeleteLibrary called for item at index {idx:?}");

        if let Some(id) = self
          .library_rows
          .iter()
          .find(|&lr| lr.index == idx)
          .map(|lr| lr.library.id)
        {
          self.libraries.retain(|lib| lib.id != id);
        }

        self.library_rows.guard().remove(idx.current_index());
      }

      PrefsMsg::ShowToast(msg, is_error) => {
        debug!("Emit toast notification: \"{}\"", &msg);

        // Add error styling
        // Note: Unicode symbol may not be visible in IDE
        let (msg, timeout) = if is_error {
          (format!("<span foreground='#e01b24'>⚠</span> {msg}"), 5)
        } else {
          (msg, 3)
        };

        let toast = adw::Toast::builder().title(msg).timeout(timeout).build();
        self.root.add_toast(toast);
      }

      PrefsMsg::CloseRequested => {
        sender
          .output(PrefsOutput::Close)
          .expect("PrefsOutput receiver dropped");
      }

      PrefsMsg::NoOp => {}
    }
  }
}

fn build_library_rows(
  libs: impl IntoIterator<Item = Library>,
  sender: &ComponentSender<PrefsModel>,
) -> FactoryVecDeque<LibraryRow> {
  let mut library_rows = FactoryVecDeque::builder()
    .launch(gtk::ListBox::builder().css_classes(["boxed-list"]).build())
    .forward(sender.input_sender(), |msg| match msg {
      LibraryRowOutput::Delete(idx) => PrefsMsg::DeleteLibraryRow(idx),
      LibraryRowOutput::FileDialogRequest(idx) => PrefsMsg::EditLibraryFileDialogRequest(idx),
      LibraryRowOutput::ShowToast(msg, is_error) => PrefsMsg::ShowToast(msg, is_error),
    });

  {
    let mut guard = library_rows.guard();
    libs.into_iter().for_each(|lib| {
      guard.push_back(lib);
    });
  }

  library_rows
}
