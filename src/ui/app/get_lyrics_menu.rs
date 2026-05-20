use std::sync::LazyLock;

use chrono::NaiveDateTime;
use diesel::{
  backend::Backend,
  deserialize::{FromSql, FromSqlRow},
  expression::AsExpression,
  serialize::ToSql,
  sql_types::Text,
  sqlite::Sqlite,
};
use relm4::{
  actions::{RelmAction, RelmActionGroup},
  prelude::*,
};
use serde::{Deserialize, Serialize};
use tracing::{error, trace};

use crate::{SETTINGS, lyrics::LyricsType, track::Track, util::now};

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
    // Restore saved settings
    let (lyrics_type_from_settings, last_checked_from_settings) = {
      let guard = SETTINGS.read();
      let lyrics_type = guard
        .as_ref()
        .map_or(Type::default(), |settings| settings.get_lyrics_menu_lyrics_type.clone());
      let last_checked = guard
        .as_ref()
        .map_or(Checked::default(), |settings| settings.get_lyrics_menu_last_checked.clone());
      (
        ron::to_string(&lyrics_type).unwrap_or_else(|_| {
          ron::to_string(&Type::default()).unwrap_or_else(|_| "NotPreferred".to_string())
        }),
        ron::to_string(&last_checked).unwrap_or_else(|_| {
          ron::to_string(&Checked::default()).unwrap_or_else(|_| "Any".to_string())
        }),
      )
    };

    let mut menu_action_group = RelmActionGroup::<GroupGetLyrics>::new();

    let sender_handle = sender.clone();
    let action_set_type: RelmAction<ActionGetLyricsMenuLyricsType> =
      RelmAction::new_stateful_with_target_value(
        &lyrics_type_from_settings,
        move |_action, state, value: String| {
          if value.as_str() == "NoLyrics" {
            sender_handle.input(GetLyricsButtonModelMsg::TypeChanged(Type::NoLyrics));
          } else {
            sender_handle.input(GetLyricsButtonModelMsg::TypeChanged(Type::NotPreferred));
          }

          *state = value;
        },
      );
    menu_action_group.add_action(action_set_type);

    let menu_lyrics_type_section = gtk::gio::Menu::new();
    menu_lyrics_type_section.append(Some("_No Lyrics"), Some("get_lyrics.lyrics_type::NoLyrics"));
    menu_lyrics_type_section
      .append(Some("Not _Preferred"), Some("get_lyrics.lyrics_type::NotPreferred"));

    let sender_handle = sender.clone();
    let action_set_last_checked: RelmAction<ActionGetLyricsMenuLastChecked> =
      RelmAction::new_stateful_with_target_value(
        &last_checked_from_settings,
        move |_action, state, value: String| {
          match value.as_str() {
            "Never" => {
              sender_handle.input(GetLyricsButtonModelMsg::CheckedChanged(Checked::Never));
            }
            "Months(1)" => {
              sender_handle.input(GetLyricsButtonModelMsg::CheckedChanged(Checked::Months(1)));
            }
            "Months(3)" => {
              sender_handle.input(GetLyricsButtonModelMsg::CheckedChanged(Checked::Months(3)));
            }
            "Months(6)" => {
              sender_handle.input(GetLyricsButtonModelMsg::CheckedChanged(Checked::Months(6)));
            }
            "Year" => {
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
    menu_time_section.append(Some("Ne_ver"), Some("get_lyrics.last_checked::Never"));
    menu_time_section.append(Some("> _1 Month"), Some("get_lyrics.last_checked::Months(1)"));
    menu_time_section.append(Some("> _3 Months"), Some("get_lyrics.last_checked::Months(3)"));
    menu_time_section.append(Some("> _6 Months"), Some("get_lyrics.last_checked::Months(6)"));
    menu_time_section.append(Some("> 1 _Year"), Some("get_lyrics.last_checked::Year"));
    menu_time_section.append(Some("_Any"), Some("get_lyrics.last_checked::Any"));

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
          ((track.lyrics.is_some() && track.lyrics_synchronised)
            || (track.lyrics.is_none() && track.lyrics_sidecar_lrc_file.is_some()))
            && track.lyrics_sidecar_txt_file.is_none()
        }
        LyricsType::Plain => {
          ((track.lyrics.is_some() && !track.lyrics_synchronised)
            || (track.lyrics.is_none() && track.lyrics_sidecar_txt_file.is_some()))
            && track.lyrics_sidecar_lrc_file.is_none()
        }
      },
    } {
      // Short circuit if the track already meets lyrics requirement;
      // there's no need to evaluate the last checked date
      return false;
    }

    match self.last_checked {
      _ if track.last_api_check_at.is_none() => true, // always match if track never checked for lyrics
      Checked::Never => track.last_api_check_at.is_none(),
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

#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, AsExpression, FromSqlRow,
)]
#[diesel(sql_type = Text)]
pub enum Type {
  NoLyrics,
  #[default]
  NotPreferred,
}

#[derive(
  Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, AsExpression, FromSqlRow,
)]
#[diesel(sql_type = Text)]
pub enum Checked {
  Never,
  Months(u32),
  Year,
  #[default]
  Any,
}

impl FromSql<Text, Sqlite> for Type {
  fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> diesel::deserialize::Result<Self> {
    let s = <String as FromSql<Text, Sqlite>>::from_sql(bytes)?;
    ron::from_str(&s).map_err(|error| {
      error!(
        "Error deserializing `GetLyricsMenuState` enum `Type` from database value \"{s}\": {error}"
      );
      error.into()
    })
  }
}

impl ToSql<Text, Sqlite> for Type {
  fn to_sql<'b>(
    &'b self,
    out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
  ) -> diesel::serialize::Result {
    out.set_value(ron::to_string(&self)?);
    Ok(diesel::serialize::IsNull::No)
  }
}

impl FromSql<Text, Sqlite> for Checked {
  fn from_sql(bytes: <Sqlite as Backend>::RawValue<'_>) -> diesel::deserialize::Result<Self> {
    let s = <String as FromSql<Text, Sqlite>>::from_sql(bytes)?;
    ron::from_str(&s).map_err(|error| {
      error!("Error deserializing `GetLyricsMenuState` enum `Checked` from database value \"{s}\": {error}");
      error.into()
    })
  }
}

impl ToSql<Text, Sqlite> for Checked {
  fn to_sql<'b>(
    &'b self,
    out: &mut diesel::serialize::Output<'b, '_, Sqlite>,
  ) -> diesel::serialize::Result {
    out.set_value(ron::to_string(&self)?);
    Ok(diesel::serialize::IsNull::No)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn filter_track_no_lyrics() {
    let mut track = Track::default();
    let state = GetLyricsMenuState {
      lyrics_type: Type::NoLyrics,
      last_checked: Checked::Any,
    };

    // No lyrics of any type - should be `true`
    assert!(state.filter_track(&track, LyricsType::Sync));

    // Any lyrics - should be `false`
    track.lyrics = Some("plain lyrics".into());
    assert!(!state.filter_track(&track, LyricsType::Sync));

    track.lyrics = None;
    track.lyrics_sidecar_txt_file = Some("plain lyrics".into());
    assert!(!state.filter_track(&track, LyricsType::Sync));
  }

  #[test]
  fn filter_track_not_preferred_lyrics() {
    let mut track = Track::default();
    let state = GetLyricsMenuState {
      lyrics_type: Type::NotPreferred,
      last_checked: Checked::Any,
    };

    // No lyrics - should be `true`
    assert!(state.filter_track(&track, LyricsType::Sync));

    // Plain lyrics with Sync preferred - should be `true`
    track.lyrics_sidecar_txt_file = Some("plain lyrics".into());
    assert!(state.filter_track(&track, LyricsType::Sync));

    track.lyrics_sidecar_txt_file = None;
    track.lyrics = Some("plain lyrics".into());
    assert!(state.filter_track(&track, LyricsType::Sync));

    // Sync lyrics with Plain preferred - should be `true`
    track.lyrics = Some("[01:00:00]sync lyrics\n".into());
    track.lyrics_synchronised = true;
    assert!(state.filter_track(&track, LyricsType::Plain));

    // Sync+Plain lyrics with Plain preferred - should be `true`
    track.lyrics_sidecar_txt_file = Some("plain lyrics".into());
    assert!(state.filter_track(&track, LyricsType::Plain));

    // Sync lyrics with Plain preferred - should be `true`
    track.lyrics = None;
    track.lyrics_synchronised = false;
    track.lyrics_sidecar_txt_file = None;
    track.lyrics_sidecar_lrc_file = Some("[01:00:00]sync lyrics\n".into());
    assert!(state.filter_track(&track, LyricsType::Plain));
  }

  #[test]
  fn filter_track_last_checked() {
    let mut track = Track::default();
    let mut state = GetLyricsMenuState {
      lyrics_type: Type::NoLyrics,
      last_checked: Checked::Any,
    };

    // No date - should be `true`
    assert!(state.filter_track(&track, LyricsType::Plain));

    // Just now with Any selected - should be `true`
    track.last_api_check_at = Some(now());
    assert!(state.filter_track(&track, LyricsType::Plain));

    // Just now with Never selected - should be `false`
    state.last_checked = Checked::Never;
    assert!(!state.filter_track(&track, LyricsType::Plain));

    // Just now with 1 Month selected - should be `false`
    state.last_checked = Checked::Months(1);
    assert!(!state.filter_track(&track, LyricsType::Plain));

    // Just now with Year selected - should be `false`
    state.last_checked = Checked::Year;
    assert!(!state.filter_track(&track, LyricsType::Plain));

    // 366 days ago with Year selected - should be `true`
    track.last_api_check_at = Some(now() - chrono::Days::new(366));
    assert!(state.filter_track(&track, LyricsType::Plain));
  }
}
