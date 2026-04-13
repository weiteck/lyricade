use adw::prelude::*;
use gtk::prelude::*;
use relm4::prelude::*;
use tracing::{debug, error};

use crate::track::Track;

pub struct ViewLyricsModel {
  track: Track,
  lyrics: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewLyricsSource {
  Tag,
  Lrc,
  Txt,
}

#[relm4::component(pub)]
impl SimpleComponent for ViewLyricsModel {
  type Input = ();
  type Output = ();
  type Init = (Track, ViewLyricsSource);

  view! {
    gtk::Window {
      set_title: Some(&format!("Lyrics - {}", &model.track.track_name)),
      set_default_size: (600, 700),

      gtk::ScrolledWindow {
        gtk::Box {
          set_orientation: gtk::Orientation::Vertical,

          gtk::TextView {
            set_expand: true,
            set_editable: false,
            set_cursor_visible: false,
            set_pixels_above_lines: 6,
            set_pixels_below_lines: 6,
            set_wrap_mode: gtk::WrapMode::Word,
            #[watch]
            set_buffer: Some(&gtk::TextBuffer::builder().text(&model.lyrics).build()),
          },
        },
      },
    },
  }

  fn init(
    (track, lyrics_source): Self::Init,
    root: Self::Root,
    sender: ComponentSender<Self>,
  ) -> ComponentParts<Self> {
    let lyrics = match lyrics_source {
      ViewLyricsSource::Tag => track
        .lyrics
        .clone()
        .unwrap_or_else(|| "No lyrics tag".into()),
      ViewLyricsSource::Lrc => track
        .lyrics_sidecar_lrc_file
        .clone()
        .unwrap_or_else(|| "No LRC sidecar file".into()),
      ViewLyricsSource::Txt => track
        .lyrics_sidecar_txt_file
        .clone()
        .unwrap_or_else(|| "No TXT sidecar file".into()),
    };

    let model = ViewLyricsModel { track, lyrics };

    let widgets = view_output!();

    ComponentParts { model, widgets }
  }

  fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {}
}
