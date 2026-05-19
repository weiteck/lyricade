use std::sync::LazyLock;

use chrono::NaiveDateTime;
use relm4::{
  actions::{RelmAction, RelmActionGroup},
  prelude::*,
};
use tracing::trace;

use crate::{lyrics::LyricsType, track::Track, util::now};

// Cache dates used for filtering Tracks
static TODAY: LazyLock<NaiveDateTime> = LazyLock::new(now);
static MONTHS_AGO_1: LazyLock<Option<NaiveDateTime>> =
  LazyLock::new(|| TODAY.checked_sub_months(chrono::Months::new(1)));
static MONTHS_AGO_3: LazyLock<Option<NaiveDateTime>> =
  LazyLock::new(|| TODAY.checked_sub_months(chrono::Months::new(3)));
static MONTHS_AGO_6: LazyLock<Option<NaiveDateTime>> =
  LazyLock::new(|| TODAY.checked_sub_months(chrono::Months::new(6)));
static YEAR_AGO: LazyLock<Option<NaiveDateTime>> =
  LazyLock::new(|| TODAY.checked_sub_months(chrono::Months::new(12)));

#[derive(Debug, Clone)]
pub(super) struct GetLyricsButtonModel {
  state: GetLyricsMenuState,
}

#[derive(Debug, Clone)]
pub(super) enum Type {
  NoLyrics,
  NotPreferred,
}

#[derive(Debug, Clone)]
pub(super) enum Checked {
  Never,
  Months(u32),
  Year,
  Any,
}

#[derive(Debug)]
pub(super) enum GetLyricsButtonModelMsg {
  TypeChanged(Type),
  CheckedChanged(Checked),
}

#[derive(Debug)]
pub(super) enum GetLyricsButtonOutput {
  GetLyricsMenuChanged(GetLyricsMenuState),
}

relm4::new_action_group!(pub(super) GroupGetLyrics, "get_lyrics");
relm4::new_stateful_action!(
  ActionGetLyricsMenuLyricsType,
  GroupGetLyrics,
  "lyrics_type",
  String,
  String
);
relm4::new_stateful_action!(
  ActionGetLyricsMenuLastChecked,
  GroupGetLyrics,
  "last_checked",
  String,
  String
);

#[relm4::component(pub(super))]
impl SimpleComponent for GetLyricsButtonModel {
  type Init = ();
  type Input = GetLyricsButtonModelMsg;
  type Output = GetLyricsButtonOutput;

  view! {
    adw::SplitButton {
      set_menu_model: Some(&menu),
    },
  }

  fn init(
    _init: Self::Init,
    root: Self::Root,
    sender: ComponentSender<Self>,
  ) -> ComponentParts<Self> {
    let mut menu_action_group = RelmActionGroup::<GroupGetLyrics>::new();

    let sender_handle = sender.clone();
    let action_set_type: RelmAction<ActionGetLyricsMenuLyricsType> =
      RelmAction::new_stateful_with_target_value(
        &"not_preferred".to_string(),
        move |_action, state, value: String| {
          if value.as_str() == "no_lyrics" {
            sender_handle.input(GetLyricsButtonModelMsg::TypeChanged(Type::NoLyrics));
          } else {
            sender_handle.input(GetLyricsButtonModelMsg::TypeChanged(Type::NotPreferred));
          }

          *state = value;
        },
      );
    menu_action_group.add_action(action_set_type);

    let menu_lyrics_type_section = gtk::gio::Menu::new();
    menu_lyrics_type_section.append(Some("_No Lyrics"), Some("get_lyrics.lyrics_type::no_lyrics"));
    menu_lyrics_type_section
      .append(Some("Not _Preferred"), Some("get_lyrics.lyrics_type::not_preferred"));

    let sender_handle = sender.clone();
    let action_set_last_checked: RelmAction<ActionGetLyricsMenuLastChecked> =
      RelmAction::new_stateful_with_target_value(
        &"any".to_string(),
        move |_action, state, value: String| {
          match value.as_str() {
            "never" => {
              sender_handle.input(GetLyricsButtonModelMsg::CheckedChanged(Checked::Never));
            }
            "months_1" => {
              sender_handle.input(GetLyricsButtonModelMsg::CheckedChanged(Checked::Months(1)));
            }
            "months_3" => {
              sender_handle.input(GetLyricsButtonModelMsg::CheckedChanged(Checked::Months(3)));
            }
            "months_6" => {
              sender_handle.input(GetLyricsButtonModelMsg::CheckedChanged(Checked::Months(6)));
            }
            "year" => {
              sender_handle.input(GetLyricsButtonModelMsg::CheckedChanged(Checked::Year));
            }
            _ => {
              sender_handle.input(GetLyricsButtonModelMsg::CheckedChanged(Checked::Any));
            }
          }

          *state = value;
        },
      );
    menu_action_group.add_action(action_set_last_checked);

    let menu_time_section = gtk::gio::Menu::new();
    menu_time_section.append(Some("Ne_ver"), Some("get_lyrics.last_checked::never"));
    menu_time_section.append(Some("> _1 Month"), Some("get_lyrics.last_checked::months_1"));
    menu_time_section.append(Some("> _3 Months"), Some("get_lyrics.last_checked::months_3"));
    menu_time_section.append(Some("> _6 Months"), Some("get_lyrics.last_checked::months_6"));
    menu_time_section.append(Some("> 1 _Year"), Some("get_lyrics.last_checked::year"));
    menu_time_section.append(Some("_Any"), Some("get_lyrics.last_checked::any"));

    let menu = gtk::gio::Menu::new();
    menu.append_section(Some("Lyrics"), &menu_lyrics_type_section);
    menu.append_section(Some("Last Checked"), &menu_time_section);

    menu_action_group.register_for_widget(&root);

    let model = GetLyricsButtonModel {
      state: GetLyricsMenuState {
        lyrics_type: Type::NotPreferred,
        last_checked: Checked::Any,
      },
    };

    let widgets = view_output!();

    ComponentParts { model, widgets }
  }

  fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
    match message {
      GetLyricsButtonModelMsg::TypeChanged(lyrics_type) => {
        trace!(
          "Get Lyrics menu: Lyrics type changed from {:?} to {:?}",
          self.state.lyrics_type, lyrics_type
        );

        self.state.lyrics_type = lyrics_type;

        sender
          .output(GetLyricsButtonOutput::GetLyricsMenuChanged(self.state.clone()))
          .expect("GetLyricsButtonOutput receiver dropped");
      }
      GetLyricsButtonModelMsg::CheckedChanged(last_checked) => {
        trace!(
          "Get Lyrics menu: Lyrics type changed from {:?} to {:?}",
          self.state.last_checked, last_checked
        );

        self.state.last_checked = last_checked;

        sender
          .output(GetLyricsButtonOutput::GetLyricsMenuChanged(self.state.clone()))
          .expect("GetLyricsButtonOutput receiver dropped");
      }
    }
  }
}

impl GetLyricsButtonModel {
  pub(super) fn state(&self) -> GetLyricsMenuState {
    self.state.clone()
  }
}

#[derive(Debug, Clone)]
pub(super) struct GetLyricsMenuState {
  pub(super) lyrics_type: Type,
  pub(super) last_checked: Checked,
}

impl GetLyricsMenuState {
  /// Returns `true` if the `Track` does not meet requirements set in the 'Get Lyrics' menu.
  pub(super) fn filter_track(&self, track: &Track, preferred_lyrics_type: LyricsType) -> bool {
    if !match self.lyrics_type {
      Type::NoLyrics => {
        !(track.lyrics.is_some()
          || track.lyrics_sidecar_lrc_file.is_some()
          || track.lyrics_sidecar_txt_file.is_some())
      }
      Type::NotPreferred => !match preferred_lyrics_type {
        LyricsType::Sync => {
          (track.lyrics.is_some() && track.lyrics_synchronised)
            || track.lyrics_sidecar_lrc_file.is_some()
        }
        LyricsType::Plain => {
          (track.lyrics.is_some() && !track.lyrics_synchronised)
            || track.lyrics_sidecar_txt_file.is_some()
        }
      },
    } {
      // Short circuit if the track already meets lyrics requirement;
      // there's no need to evaluate the last checked date
      return false;
    }

    match self.last_checked {
      _ if track.last_api_check_at.is_none() => true, // always match if track never checked for lyrics
      Checked::Months(months) if let Some(last_checked) = track.last_api_check_at => match months {
        1 => MONTHS_AGO_1.is_some_and(|cutoff| last_checked < cutoff),
        3 => MONTHS_AGO_3.is_some_and(|cutoff| last_checked < cutoff),
        _ => MONTHS_AGO_6.is_some_and(|cutoff| last_checked < cutoff),
      },
      Checked::Year
        if let Some(last_checked) = track.last_api_check_at
          && let Some(cutoff) = *YEAR_AGO =>
      {
        last_checked < cutoff
      }
      _ => true, // handle `Any` variant
    }
  }
}
