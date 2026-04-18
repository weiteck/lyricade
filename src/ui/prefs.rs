use std::{collections::HashSet, path::PathBuf};

use adw::prelude::*;
use camino::Utf8PathBuf;
use relm4::{gtk::EventControllerKey, prelude::*};
use relm4_components::open_dialog::*;
use tracing::{debug, trace};

use crate::{
  SETTINGS,
  library::Library,
  lyrics::LyricsType,
  settings::Settings,
  ui::prefs::library_row::{LibraryRow, LibraryRowOutput},
  util::{self, now},
};

mod library_row;

pub struct PrefsModel {
  libraries: HashSet<Library>,
  library_rows: FactoryVecDeque<LibraryRow>,

  settings_initial: Settings,
  settings_current: Settings,
  settings_default: Settings,

  file_dialog: Controller<OpenDialog>,
}

#[derive(Debug)]
pub enum PrefsMsg {
  DefaultSettings,
  RevertSettings,
  SaveSettings,
  UpdateSetting(ExposedSetting),

  OpenFileDialogRequest,
  OpenFileDialogResponse(PathBuf),

  EditLibrary(DynamicIndex),
  DeleteLibrary(DynamicIndex),

  NoOp,
}

#[derive(Debug)]
pub enum PrefsOutput {
  RebuildTracksTable,
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
  type Init = Vec<Library>;

  view! {
    prefs_window = gtk::Window {
      set_title: Some("Preferences"),
      set_default_size: (600, 600),

      #[wrap(Some)]
      set_titlebar = &adw::HeaderBar {
        pack_end = &gtk::Button {
          set_tooltip: "Restore Settings",
          set_icon_name: "document-revert-symbolic",
          #[watch]
          set_sensitive: model.settings_current != model.settings_initial,
          connect_clicked => PrefsMsg::RevertSettings,
        },
        pack_end = &gtk::Button {
          set_tooltip: "Apply Default Settings",
          set_icon_name: "folder-documents-symbolic",
          #[watch]
          set_sensitive: model.settings_current != model.settings_default,
          connect_clicked => PrefsMsg::DefaultSettings,
        },
      },

      // Update and save settings on close
      connect_close_request[sender] => move |_| {
        sender.input(PrefsMsg::SaveSettings);
        gtk::glib::Propagation::Proceed
      },

      gtk::ScrolledWindow {
        gtk::Box {
          set_orientation: gtk::Orientation::Vertical,
          set_margin_vertical: 24,
          set_margin_horizontal: 48,
          set_spacing: 24,

          adw::PreferencesGroup {
            set_title: "Music Libraries",
            set_description: Some("Add, remove or edit Music Libraries. A Music Library is just a path on which to search for audio files."),

            #[local_ref]
            libraries_list_box -> gtk::ListBox {
              set_selection_mode: gtk::SelectionMode::None,
              add_css_class: "boxed-list",

              // Add library button
              adw::ActionRow {
                set_title: "Add Music Library",
                set_halign: gtk::Align::Fill,
                set_hexpand: true,
                set_activatable: true,
                add_css_class: "button",
                set_activatable_widget: Some(&add_row_widget),
                connect_activated => PrefsMsg::OpenFileDialogRequest,

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
                    set_label: "Add Music Library",
                    add_css_class: "title",
                  }
                },
              },
            },
          },

          adw::PreferencesGroup {
            set_title: "Preferred Lyrics Format",
            set_description: Some("Choose what lyrics format to prefer when fetching lyrics online or cleaning up multiple sidecar files."),

            gtk::ListBox {
              add_css_class: "boxed-list",

              adw::ActionRow {
                set_title: "Synchronous",
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
                set_title: "Plain",
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
            set_description: Some("How audio and lyrics files are scanned and managed."),

            adw::SwitchRow {
              set_title: "Ignore Unchanged",
              set_subtitle: "Only scan new or modified files",
              #[watch]
              set_active: model.settings_current.scan_new_files_only,
              connect_active_notify[sender] => move |btn| {
                sender.input(PrefsMsg::UpdateSetting(ExposedSetting::ScanNewFilesOnly(btn.is_active())));
              }
            },

            adw::SwitchRow {
              set_title: "Upgrade Lyrics Tag From Sidecar",
              set_subtitle: "Upgrade lyrics tags to the preferred format if a sidecar file of that format exists",
              #[watch]
              set_active: model.settings_current.upgrade_lyrics_tag_on_scan,
              connect_active_notify[sender] => move |btn| {
                sender.input(PrefsMsg::UpdateSetting(ExposedSetting::UpgradeLyricsTagOnScan(btn.is_active())));
              }
            },

            adw::ComboRow {
              set_title: "Clean Up Sidecar Files",
              #[watch]
              set_subtitle: if model.settings_current.delete_sidecar_files_on_scan {
                "All sibling files with the same name as an audio file but with a “.lrc” or “.txt” extension will be deleted"
              } else if model.settings_current.keep_one_sidecar_file_on_scan  {
                "Keep only the preferred lyrics format if both sync and plain sidecar files are present"
              } else {
                "Keep all sidecar files"
              },
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
            set_title: "Fetch Lyrics",
            set_description: Some("Choose what to do with the lyrics sourced from <i>lrclib.net</i>"),

            adw::SwitchRow {
              set_title: "Write to Lyrics Tag",
              set_subtitle: "Update audio file metadata with the found lyrics",
              #[watch]
              set_active: model.settings_current.update_lyrics_tag_on_fetch,
              connect_active_notify[sender] => move |btn| {
                sender.input(PrefsMsg::UpdateSetting(ExposedSetting::UpdateLyricsTagOnFetch(btn.is_active())));
              }
            },

            adw::SwitchRow {
              set_title: "Write to Sidecar File",
              set_subtitle: "Save a LRC or TXT file with the found lyrics alongside the audio file",
              #[watch]
              set_active: model.settings_current.save_sidecar_file_on_fetch,
              connect_active_notify[sender] => move |btn| {
                sender.input(PrefsMsg::UpdateSetting(ExposedSetting::SaveSidecarOnFetch(btn.is_active())));
              },

            },
          },

          adw::PreferencesGroup {
            set_title: "Date and Time Format",
            set_description: Some("Choose how dates and times and displayed in the track list view."),

            gtk::ListBox {
              add_css_class: "boxed-list",

              adw::ActionRow {
                set_title: "Simple",
                set_subtitle: &format!("Examples: “{}”, “{}”", example_datetime_simple1, example_datetime_simple2),
                set_selectable: false,

                set_activatable_widget: Some(&group_datetime_format_button_simple),
                #[name = "group_datetime_format_button_simple"]
                add_prefix = &gtk::CheckButton {
                  #[watch]
                  set_active: !model.settings_current.prefer_iso_timestamps,
                  connect_toggled[sender] => move |btn| {
                    sender.input(PrefsMsg::UpdateSetting(ExposedSetting::PreferIsoTimestamps(!btn.is_active())));
                  },
                },
              },

              adw::ActionRow {
                set_title: "Accurate",
                set_subtitle: &format!("Examples: “{}”, “{}”", example_datetime_accurate1, example_datetime_accurate2),
                set_selectable: false,

                set_activatable_widget: Some(&group_datetime_format_button_accurate),
                #[name = "group_datetime_format_button_accurate"]
                add_prefix = &gtk::CheckButton {
                  set_group: Some(&group_datetime_format_button_simple),
                  #[watch]
                  set_active: model.settings_current.prefer_iso_timestamps,
                  connect_toggled[sender] => move |btn| {
                    sender.input(PrefsMsg::UpdateSetting(ExposedSetting::PreferIsoTimestamps(btn.is_active())));
                  },
                },
              },
            },
          },
        },
      },
    },
  }

  fn init(
    libraries: Self::Init,
    root: Self::Root,
    sender: ComponentSender<Self>,
  ) -> ComponentParts<Self> {
    // Recent datetime to use as example in interface
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

    let mut library_rows = FactoryVecDeque::builder()
      .launch(gtk::ListBox::builder().css_classes(["boxed-list"]).build())
      .forward(sender.input_sender(), |msg| match msg {
        LibraryRowOutput::Delete(idx) => PrefsMsg::DeleteLibrary(idx),
        LibraryRowOutput::Edit(idx) => PrefsMsg::EditLibrary(idx),
      });

    {
      let mut guard = library_rows.guard();
      libraries.clone().into_iter().for_each(|lib| {
        guard.push_back(lib);
      });
    }

    // Create file dialog
    let mut file_dialog_settings = OpenDialogSettings::default();
    file_dialog_settings.create_folders = false;
    file_dialog_settings.folder_mode = true;
    file_dialog_settings.is_modal = true;
    file_dialog_settings.accept_label = "Add Folder".into();
    let file_dialog = OpenDialog::builder()
      .transient_for_native(&root)
      .launch(file_dialog_settings)
      .forward(sender.input_sender(), |resp| match resp {
        OpenDialogResponse::Accept(path) => PrefsMsg::OpenFileDialogResponse(path),
        OpenDialogResponse::Cancel => PrefsMsg::NoOp,
      });

    let model = {
      let settings = SETTINGS.read().expect("settings lock is poisoned");
      PrefsModel {
        libraries,
        library_rows,
        settings_initial: settings.clone(),
        settings_current: settings.clone(),
        settings_default: Settings::default(),
        file_dialog,
      }
    };

    let libraries_list_box = model.library_rows.widget();
    let widgets = view_output!();

    // Handle key presses
    let sender_handle = sender.clone();
    let controller = EventControllerKey::new();
    controller.connect_key_pressed(move |_con, key, _idx, modifier| {
      trace!("Prefs key event: key {key} + {:?}", modifier);
      if key == gtk::gdk::Key::Escape {
        sender_handle
          .output(PrefsOutput::Close)
          .expect("PrefsOutput receiver dropped");
      }
      gtk::glib::Propagation::Proceed
    });
    root.add_controller(controller);

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
        let mut settings = SETTINGS.write().expect("settings lock is poisoned");
        *settings = self.settings_current.clone();
        settings.save().expect("unable to save settings");

        sender
          .output(PrefsOutput::Close)
          .expect("PrefsOutput receiver dropped");
      }

      PrefsMsg::UpdateSetting(setting) => match setting {
        ExposedSetting::PreferLyricsType(lyrics_type) => {
          debug!("UpdateSetting: PreferLyricsType: {lyrics_type}");
          self.settings_current.prefer_lyrics_type = lyrics_type
        }
        ExposedSetting::PreferIsoTimestamps(active) => {
          debug!("UpdateSetting: PreferIsoTimestamps: {active}");
          self.settings_current.prefer_iso_timestamps = active;

          // We have to update the singleton immediately for the tracks table to reflect the change
          {
            let mut settings = SETTINGS.write().expect("settings lock is poisoned");
            *settings = self.settings_current.clone();
          }

          // Trigger table rebuild
          sender
            .output(PrefsOutput::RebuildTracksTable)
            .expect("PrefsOutput receiver dropped");
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
        }
        ExposedSetting::SaveSidecarOnFetch(active) => {
          debug!("UpdateSetting: SaveSidecarFileOnFetch: {active}");
          self.settings_current.save_sidecar_file_on_fetch = active;
        }
      },

      PrefsMsg::OpenFileDialogRequest => {
        debug!("Opening file dialog");
        self.file_dialog.emit(OpenDialogMsg::Open);
      }

      // TODO: Use alert dialog for errors
      PrefsMsg::OpenFileDialogResponse(path) => {
        debug!("Adding library path: {}", path.to_string_lossy());
        if let Ok(path) = Utf8PathBuf::from_path_buf(path) {
          let added = Library::add(&path).expect("unable to add library");
          self.library_rows.guard().push_back(added.clone());
          self.libraries.insert(added);
        }
      }

      PrefsMsg::EditLibrary(idx) => {
        debug!("EditLibrary called for item at index {idx:?}");
      }

      // TODO: Show toast with 'undo' button
      PrefsMsg::DeleteLibrary(idx) => {
        debug!("DeleteLibrary called for item at index {idx:?}");

        if let Some(id) = self
          .library_rows
          .iter()
          .find(|&lr| lr.index == idx)
          .map(|lr| lr.library.id)
        {
          if let Some(lib) = self.libraries.iter().find(|lib| lib.id == id) {
            lib.remove().expect("failed to delete {lib}");
            debug!("Deleted {lib}");

            self.libraries.retain(|lib| lib.id != id);
          }
        }

        self.library_rows.guard().remove(idx.current_index());
      }

      PrefsMsg::NoOp => {}
    }
  }
}
