use std::{collections::HashSet, path::PathBuf};

use adw::prelude::*;
use camino::Utf8PathBuf;
use relm4::{
  adw::PreferencesDialog,
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
  settings::{ColourScheme, Settings},
  ui::prefs::library_row::{LibraryRow, LibraryRowMsg, LibraryRowOutput},
  util::{self, now},
};

mod library_row;

pub(crate) struct PrefsModel {
  root: PreferencesDialog,

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
pub(crate) enum PrefsMsg {
  DefaultSettings,
  RevertSettings,
  SaveAndClose,
  UpdateSetting(ExposedSetting),

  AddLibraryFileDialogRequest,
  AddLibraryFileDialogResponse(PathBuf),

  EditLibraryFileDialogRequest(DynamicIndex),
  EditLibraryFileDialogResponse(PathBuf),

  DeleteLibraryRow(DynamicIndex),

  ShowToast(String, bool),

  NoOp,
}

#[derive(Debug)]
pub(crate) enum PrefsOutput {
  Close(RebuildTracksTableRequired), // request parent window to close Prefs window
}

#[derive(Debug)]
pub(crate) struct RebuildTracksTableRequired(pub(crate) bool);

#[derive(Debug)]
pub(crate) enum ExposedSetting {
  PreferLyricsType(LyricsType),

  ScanNewFilesOnly(bool),

  UpdateLyricsTagOnFetch(bool),
  SaveSidecarOnFetch(bool),

  ColourScheme(ColourScheme),
  ColumnSeparators(bool),
  RowSeparators(bool),

  PreferIsoTimestamps(bool),

  // Advanced settings
  PlainLyricsUsltFrame(bool),
}

#[relm4::component(pub)]
impl SimpleComponent for PrefsModel {
  type Input = PrefsMsg;
  type Output = PrefsOutput;
  type Init = (Settings, Vec<Library>);

  view! {
    prefs_window = adw::PreferencesDialog {
      set_title: "Preferences",

      // Update and save settings on close
      connect_closed[sender] => move |_| {
        sender.input(PrefsMsg::SaveAndClose);
      },

      add = &adw::PreferencesPage {
        set_title: "_General",
        set_use_underline: true,
        set_icon_name: Some("preferences-system-symbolic"),

        adw::PreferencesGroup {
          set_title: "Lyrics Format",

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
              set_subtitle: "Prefer plain text lyrics",
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
          set_title: "Saving Lyrics",

          adw::SwitchRow {
            set_title: "Update Lyrics _Tags",
            set_use_underline: true,
            set_subtitle: "Update audio file metadata",
            #[watch]
            set_active: model.settings_current.update_lyrics_tag_on_fetch,
            connect_active_notify[sender] => move |btn| {
              sender.input(PrefsMsg::UpdateSetting(ExposedSetting::UpdateLyricsTagOnFetch(btn.is_active())));
            }
          },

          adw::SwitchRow {
            set_title: "Save Sidecar _Files",
            set_use_underline: true,
            set_subtitle: "Save .lrc or .txt lyrics files alongside audio files",
            #[watch]
            set_active: model.settings_current.save_sidecar_file_on_fetch,
            connect_active_notify[sender] => move |btn| {
              sender.input(PrefsMsg::UpdateSetting(ExposedSetting::SaveSidecarOnFetch(btn.is_active())));
            },
          },
        },

        adw::PreferencesGroup {
          set_title: "Library Scan",

          adw::SwitchRow {
            set_title: "Ignore _Unchanged",
            set_use_underline: true,
            set_subtitle: "Only scan added or modified files when refreshing tracks",
            #[watch]
            set_active: model.settings_current.scan_new_files_only,
            connect_active_notify[sender] => move |btn| {
              sender.input(PrefsMsg::UpdateSetting(ExposedSetting::ScanNewFilesOnly(btn.is_active())));
            }
          },
        },

        adw::PreferencesGroup {
          set_title: "Appearance",

          adw::ComboRow {
            set_title: "C_olour Scheme",
            set_use_underline: true,
            set_model: Some(&gtk::StringList::new(&[
              "Follow System",
              "Light",
              "Dark",
            ])),
            #[watch]
            set_selected: model.settings_current.colour_scheme as u32,
            connect_selected_item_notify[sender] => move |row| {
              match row.selected() {
                0 => {
                  sender.input(PrefsMsg::UpdateSetting(ExposedSetting::ColourScheme(ColourScheme::System)));
                }
                1 => {
                  sender.input(PrefsMsg::UpdateSetting(ExposedSetting::ColourScheme(ColourScheme::Light)));
                }
                2.. => {
                  sender.input(PrefsMsg::UpdateSetting(ExposedSetting::ColourScheme(ColourScheme::Dark)));
                }
              }
            },
          },

          adw::SwitchRow {
            set_title: "_Column Separators",
            set_use_underline: true,
            #[watch]
            set_active: model.settings_current.tracks_table_col_separators,
            connect_active_notify[sender] => move |btn| {
              sender.input(PrefsMsg::UpdateSetting(ExposedSetting::ColumnSeparators(btn.is_active())));
            }
          },

          adw::SwitchRow {
            set_title: "_Row Separators",
            set_use_underline: true,
            #[watch]
            set_active: model.settings_current.tracks_table_row_separators,
            connect_active_notify[sender] => move |btn| {
              sender.input(PrefsMsg::UpdateSetting(ExposedSetting::RowSeparators(btn.is_active())));
            }
          },
        },

        adw::PreferencesGroup {
          set_title: "Date and Time Format",

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
          adw::ExpanderRow {
            set_focusable: false,
            set_selectable: false,
            set_title: "Advanced Settings",

            add_row = &adw::SwitchRow {
              set_title: "Plain Lyrics in ID3v2 USLT Frame (MP3)",
              set_use_underline: true,
              set_subtitle: "Synchronous lyrics will be encoded in the SYLT synchronised text frame per the ID3v2 V3 spec, but a copy will also be inserted in the USLT unsynchronised text frame. Choose whether this copy is converted to plain text format or LRC sync format is retained. <b>It is recommended to keep this option disabled to increase player compatibility.</b>",
              #[watch]
              set_active: model.settings_current.plain_lyrics_in_id3v2_uslt_frame,
              connect_active_notify[sender] => move |btn| {
                sender.input(PrefsMsg::UpdateSetting(ExposedSetting::PlainLyricsUsltFrame(btn.is_active())));
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
        sender_handle.input(PrefsMsg::SaveAndClose);
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

      PrefsMsg::SaveAndClose => {
        if self.settings_current != self.settings_initial {
          if let Ok(mut guard) = SETTINGS.write() {
            *guard = self.settings_current.clone();
            let _ = guard.save();
          } else {
            error!("Settings lock was poisoned while closing Preferences");
          }
        }

        let rebuild_required = self.settings_initial.prefer_accurate_timestamps
          != self.settings_current.prefer_accurate_timestamps;

        sender
          .output(PrefsOutput::Close(RebuildTracksTableRequired(rebuild_required)))
          .expect("PrefsOutput receiver dropped");
      }

      PrefsMsg::UpdateSetting(setting) => match setting {
        ExposedSetting::PreferLyricsType(lyrics_type) => {
          debug!("UpdateSetting: PreferLyricsType: {lyrics_type}");
          self.settings_current.prefer_lyrics_type = lyrics_type;
        }
        ExposedSetting::ScanNewFilesOnly(active) => {
          debug!("UpdateSetting: ScanNewFilesOnly: {active}");
          self.settings_current.scan_new_files_only = active;
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
        ExposedSetting::ColourScheme(colour_scheme) => {
          debug!("UpdateSetting: ColourScheme: {colour_scheme:?}");
          self.settings_current.colour_scheme = colour_scheme;

          adw::StyleManager::default().set_color_scheme(
            match self.settings_current.colour_scheme {
              ColourScheme::System => adw::ColorScheme::Default,
              ColourScheme::Light => adw::ColorScheme::ForceLight,
              ColourScheme::Dark => adw::ColorScheme::ForceDark,
            },
          );
        }
        ExposedSetting::ColumnSeparators(active) => {
          debug!("UpdateSetting: ColumnSeparators: {active}");
          self.settings_current.tracks_table_col_separators = active;
        }
        ExposedSetting::RowSeparators(active) => {
          debug!("UpdateSetting: RowSeparators: {active}");
          self.settings_current.tracks_table_row_separators = active;
        }
        ExposedSetting::PreferIsoTimestamps(active) => {
          debug!("UpdateSetting: PreferIsoTimestamps: {active}");
          self.settings_current.prefer_accurate_timestamps = active;
        }
        ExposedSetting::PlainLyricsUsltFrame(active) => {
          debug!("UpdateSetting: PlainLyricsUsltFrame: {active}");
          self.settings_current.plain_lyrics_in_id3v2_uslt_frame = active;
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
