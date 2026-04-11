use adw::prelude::*;
use gtk::prelude::*;
use relm4::prelude::*;
use tracing::{debug, error};

use crate::{SETTINGS, lyrics::LyricsType, settings::Settings};

pub struct PrefsModel {
  settings_initial: Settings,
  settings_current: Settings,
  settings_default: Settings,
}

#[derive(Debug)]
pub enum PrefsMsg {
  DefaultSettings,
  RevertSettings,
  SaveSettings,
  UpdateSetting(ExposedSetting),
}

#[derive(Debug)]
pub enum PrefsOutput {
  RebuildTracksTable,
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
  type Init = ();

  view! {
    gtk::Window {
      set_title: Some("Preferences"),
      set_default_size: (600, 700),

      #[wrap(Some)]
      set_titlebar = &adw::HeaderBar {
        pack_end = &gtk::Button {
          set_tooltip: "Undo Changes",
          set_icon_name: "document-revert-symbolic",
          #[watch]
          set_sensitive: model.settings_current != model.settings_initial,
          connect_clicked => PrefsMsg::RevertSettings,
        },
        pack_end = &gtk::Button {
          set_tooltip: "Use Defaults",
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
          set_margin_all: 24,
          set_spacing: 24,

          adw::PreferencesGroup {
            set_title: "General",

            adw::ComboRow {
              set_title: "Preferred Lyrics Format",
              set_subtitle: "Favour synchronous or plain lyrics format, where available, when fetching lyrics or managing sidecar files",
              set_model: Some(&gtk::StringList::new(&[
                "Sync",
                "Plain",
              ])),
              #[watch]
              set_selected: if model.settings_current.prefer_lyrics_type == LyricsType::Sync { 0 } else { 1 },
              connect_selected_item_notify[sender] => move |row| {
                let prefers = if row.selected() == 0 {
                  LyricsType::Sync
                } else {
                  LyricsType::Plain
                };
                sender.input(PrefsMsg::UpdateSetting(ExposedSetting::PreferLyricsType(prefers)));
              },
            },

            adw::ComboRow {
              set_title: "Date and Time Style",
              #[watch]
              set_subtitle: if model.settings_current.prefer_iso_timestamps {
                "Example: “2026-01-30 18:05:10”"
              } else {
                "Example: “3 weeks ago”"
              },
              set_model: Some(&gtk::StringList::new(&[
                "Simple",
                "Accurate",
              ])),
              #[watch]
              set_selected: if model.settings_current.prefer_iso_timestamps { 1 } else { 0 },
              connect_selected_item_notify[sender] => move |row| {
                sender.input(PrefsMsg::UpdateSetting(ExposedSetting::PreferIsoTimestamps(row.selected() == 1)));
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
            set_description: Some("What to do with the lyrics sourced from <i>lrclib.net</i>"),

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
        },
      },
    },
  }

  fn init(
    _init: Self::Init,
    root: Self::Root,
    sender: ComponentSender<Self>,
  ) -> ComponentParts<Self> {
    let model = {
      let settings = SETTINGS.read().expect("settings lock is poisoned");
      PrefsModel {
        settings_initial: settings.clone(),
        settings_current: settings.clone(),
        settings_default: Settings::default(),
      }
    };
    let widgets = view_output!();
    ComponentParts { model, widgets }
  }

  fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
    match message {
      PrefsMsg::DefaultSettings => {
        self.settings_current = self.settings_default.clone();
      }

      PrefsMsg::RevertSettings => {
        self.settings_current = self.settings_initial.clone();
      }

      PrefsMsg::SaveSettings => {
        let mut settings = SETTINGS.write().expect("settings lock is poisoned");
        *settings = self.settings_current.clone();
        settings
          .save()
          .inspect_err(|e| error!("Error saving updated Settings: {e}"))
          .unwrap();
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
    }
  }
}
