use adw::prelude::*;
use gtk::prelude::*;
use relm4::prelude::*;
use tracing::{debug, error};

use crate::{SETTINGS, lyrics::LyricsType, settings::Settings};

pub struct PrefsModel {
  settings_initial: Settings,
}

#[derive(Debug)]
pub enum PrefsMsg {
  UpdateSetting(ExposedSetting),
  SaveSettings,
}

#[derive(Debug)]
pub enum ExposedSetting {
  PreferLyricsType(LyricsType),
  PreferIsoTimestamps(bool),

  ScanNewFilesOnly(bool),
  UpgradeLyricsTagOnScan(bool),
  DeleteSidecarFilesOnScan(bool),
  KeepOneSidecarFileOnScan(bool),

  UpdateLyricsTagOnFetch(bool),
  SaveSidecarFileOnFetch(bool),
}

#[relm4::component(pub)]
impl SimpleComponent for PrefsModel {
  type Input = PrefsMsg;
  type Output = ();
  type Init = ();

  view! {
    gtk::Window {
      set_title: Some("Preferences"),
      set_default_size: (600, 700),

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
              set_subtitle: "Favour synchronous (LRC) or plain (TXT) lyrics format, if available.",
              set_model: Some(&gtk::StringList::new(&[
                "Sync",
                "Plain",
              ])),
              set_selected: if model.settings_initial.prefer_lyrics_type == LyricsType::Sync { 0 } else { 1 },
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
              set_title: "Preferred Date and Time Style",
              #[watch]
              set_subtitle: if model.settings_initial.prefer_iso_timestamps {
                "Example: “2026-01-30 18:05:10”"
              } else {
                "Example: “3 weeks ago”"
              },
              set_model: Some(&gtk::StringList::new(&[
                "Simple",
                "Accurate",
              ])),
              set_selected: if model.settings_initial.prefer_iso_timestamps { 1 } else { 0 },
              connect_selected_item_notify[sender] => move |row| {
                sender.input(PrefsMsg::UpdateSetting(ExposedSetting::PreferIsoTimestamps(row.selected() == 1)));
              },
            },
          },

          adw::PreferencesGroup {
            set_title: "Music Library",
            set_description: Some("How audio and lyrics files are scanned and managed."),

            adw::SwitchRow {
              set_title: "Ignore Unchanged",
              set_subtitle: "Only scan new or modified files.",
              set_active: model.settings_initial.scan_new_files_only,
              connect_active_notify[sender] => move |btn| {
                sender.input(PrefsMsg::UpdateSetting(ExposedSetting::ScanNewFilesOnly(btn.is_active())));
              }
            },

            adw::SwitchRow {
              set_title: "Upgrade Lyrics Tag From Sidecar",
              set_subtitle: "Upgrade lyrics tags to the preferred format if a sidecar file of that format exists.",
              set_active: model.settings_initial.upgrade_lyrics_tag_on_scan,
              connect_active_notify[sender] => move |btn| {
                sender.input(PrefsMsg::UpdateSetting(ExposedSetting::UpgradeLyricsTagOnScan(btn.is_active())));
              }
            },

            adw::ComboRow {
              set_title: "Clean Up Sidecar Files",
              #[watch]
              set_subtitle: if model.settings_initial.delete_sidecar_files_on_scan {
                "All sibling files with the same name as an audio file but with a “.lrc” or “.txt” extension will be deleted."
              } else if model.settings_initial.keep_one_sidecar_file_on_scan  {
                "Keep only the preferred lyrics format if both sync and plain sidecar files are present."
              } else {
                ""
              },
              set_model: Some(&gtk::StringList::new(&[
                "Do Nothing",
                "Keep One",
                "Delete",
              ])),
              set_selected: if model.settings_initial.delete_sidecar_files_on_scan { 2 }
                else if model.settings_initial.keep_one_sidecar_file_on_scan { 1 }
                else { 0 },
              connect_selected_item_notify[sender] => move |row| {
                match row.selected() {
                  1 => {
                    sender.input(PrefsMsg::UpdateSetting(ExposedSetting::DeleteSidecarFilesOnScan(false)));
                    sender.input(PrefsMsg::UpdateSetting(ExposedSetting::KeepOneSidecarFileOnScan(true)));
                  }
                  2 => {
                    sender.input(PrefsMsg::UpdateSetting(ExposedSetting::DeleteSidecarFilesOnScan(true)));
                    sender.input(PrefsMsg::UpdateSetting(ExposedSetting::KeepOneSidecarFileOnScan(false)));
                  }
                  _ => {
                    sender.input(PrefsMsg::UpdateSetting(ExposedSetting::DeleteSidecarFilesOnScan(false)));
                    sender.input(PrefsMsg::UpdateSetting(ExposedSetting::KeepOneSidecarFileOnScan(false)));
                  }
                }
              },
            },
          },

          adw::PreferencesGroup {
            set_title: "Fetching Lyrics",
            set_description: Some("What to do with the lyrics sourced from <i>lrclib.net</i>"),

            adw::SwitchRow {
              set_title: "Write to Lyrics Tag",
              set_subtitle: "Update audio file metadata with the found lyrics.",
              set_active: model.settings_initial.update_lyrics_tag_on_fetch,
              connect_active_notify[sender] => move |btn| {
                sender.input(PrefsMsg::UpdateSetting(ExposedSetting::UpdateLyricsTagOnFetch(btn.is_active())));
              }
            },

            adw::SwitchRow {
              set_title: "Write to Sidecar File",
              set_subtitle: "Save a LRC or TXT file with the found lyrics alongside the audio file.",
              set_active: model.settings_initial.update_lyrics_tag_on_fetch,
              connect_active_notify[sender] => move |btn| {
                sender.input(PrefsMsg::UpdateSetting(ExposedSetting::SaveSidecarFileOnFetch(btn.is_active())));
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
    let settings_initial = { SETTINGS.read().expect("settings lock was poisoned").clone() };
    let model = PrefsModel { settings_initial };
    let widgets = view_output!();
    ComponentParts { model, widgets }
  }

  fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
    match message {
      PrefsMsg::UpdateSetting(setting) => match setting {
        ExposedSetting::PreferLyricsType(lyrics_type) => {
          debug!("UpdateSetting: PreferLyricsType: {lyrics_type}");
          self.settings_initial.prefer_lyrics_type = lyrics_type
        }
        ExposedSetting::PreferIsoTimestamps(active) => {
          debug!("UpdateSetting: PreferIsoTimestamps: {active}");
          self.settings_initial.prefer_iso_timestamps = active;
        }
        ExposedSetting::ScanNewFilesOnly(active) => {
          debug!("UpdateSetting: ScanNewFilesOnly: {active}");
          self.settings_initial.scan_new_files_only = active;
        }
        ExposedSetting::UpgradeLyricsTagOnScan(active) => {
          debug!("UpdateSetting: UpgradeLyricsTagOnScan: {active}");
          self.settings_initial.upgrade_lyrics_tag_on_scan = active;
        }
        ExposedSetting::DeleteSidecarFilesOnScan(active) => {
          debug!("UpdateSetting: DeleteSidecarFilesOnScan: {active}");
          self.settings_initial.delete_sidecar_files_on_scan = active;
        }
        ExposedSetting::KeepOneSidecarFileOnScan(active) => {
          debug!("UpdateSetting: KeepOneSidecarFileOnScan: {active}");
          self.settings_initial.keep_one_sidecar_file_on_scan = active;
        }
        ExposedSetting::UpdateLyricsTagOnFetch(active) => {
          debug!("UpdateSetting: UpdateLyricsTagOnFetch: {active}");
          self.settings_initial.update_lyrics_tag_on_fetch = active;
        }
        ExposedSetting::SaveSidecarFileOnFetch(active) => {
          debug!("UpdateSetting: SaveSidecarFileOnFetch: {active}");
          self.settings_initial.save_sidecar_file_on_fetch = active;
        }
      },

      PrefsMsg::SaveSettings => {
        let mut settings = SETTINGS.write().expect("settings lock was poisoned");
        *settings = self.settings_initial.clone();
        settings
          .save()
          .inspect_err(|e| error!("Error saving updated Settings: {e}"))
          .unwrap();
      }
    }
  }
}
