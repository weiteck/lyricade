use adw::prelude::*;
use gtk::prelude::*;
use relm4::{RelmObjectExt, prelude::*};
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
      set_default_size: (600, 600),

      // Update and save settings on close
      connect_close_request[sender] => move |_| {
        sender.input(PrefsMsg::SaveSettings);
        gtk::glib::Propagation::Proceed
      },

      gtk::Box {
        set_orientation: gtk::Orientation::Vertical,
        set_margin_all: 24,
        set_spacing: 24,

        adw::PreferencesGroup {
          set_title: "General",
          adw::SwitchRow {
            set_title: "Use Simple Date and Time Format",
            set_subtitle: "Example: 3 weeks ago",
            set_active: !model.settings_initial.prefer_iso_timestamps,
            connect_active_notify[sender] => move |btn| {
              sender.input(PrefsMsg::UpdateSetting(ExposedSetting::PreferIsoTimestamps(btn.is_active())));
            }
          }
        },

        adw::PreferencesGroup {
          set_title: "Scan",
          adw::SwitchRow {
            set_title: "Scan New Files Only",
            set_active: model.settings_initial.scan_new_files_only,
            connect_active_notify[sender] => move |btn| {
              sender.input(PrefsMsg::UpdateSetting(ExposedSetting::ScanNewFilesOnly(btn.is_active())));
            }
          }
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

  fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
    match message {
      PrefsMsg::UpdateSetting(setting) => match setting {
        ExposedSetting::PreferLyricsType(lyrics_type) => todo!(),
        ExposedSetting::PreferIsoTimestamps(active) => {
          debug!("UpdateSetting: PreferIsoTimestamps: {active}");
          self.settings_initial.prefer_iso_timestamps = active;
        }
        ExposedSetting::ScanNewFilesOnly(active) => {
          debug!("UpdateSetting: ScanNewFilesOnly: {active}");
          self.settings_initial.scan_new_files_only = active;
        }
        ExposedSetting::UpgradeLyricsTagOnScan(active) => todo!(),
        ExposedSetting::DeleteSidecarFilesOnScan(active) => todo!(),
        ExposedSetting::KeepOneSidecarFileOnScan(active) => todo!(),
        ExposedSetting::UpdateLyricsTagOnFetch(active) => todo!(),
        ExposedSetting::SaveSidecarFileOnFetch(active) => todo!(),
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
