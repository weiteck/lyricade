use adw::prelude::*;
use relm4::{gtk::EventControllerKey, prelude::*};
use tracing::trace;

use crate::{
  lyrics::lyrics_line::LyricsLine, track::Track, ui::view_lyrics::view_lyrics_line::ViewLyricsLine,
};

pub mod view_lyrics_line;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewLyricsSource {
  Tag,
  Lrc,
  Txt,
}

pub struct ViewLyricsModel {
  track: Track,
  lyrics_lines: FactoryVecDeque<ViewLyricsLine>,
}

#[derive(Debug)]
pub enum ViewLyricsOutput {
  Close,
}

#[relm4::component(pub)]
impl SimpleComponent for ViewLyricsModel {
  type Input = ();
  type Output = ViewLyricsOutput;
  type Init = (Box<Track>, ViewLyricsSource);

  view! {
    gtk::Window {
      set_title: Some(&format!("“{}” - {}", &model.track.track_name, &model.track.artist_name)),
      set_default_size: (600, 700),

      gtk::ScrolledWindow {
        #[local_ref]
        lyrics_lines_box -> gtk::Box {
          set_css_classes: &["view-lyrics", "container"],
          set_align: gtk::Align::Fill,
          set_expand: true,
          set_margin_all: 24,
        },
      },
    },
  }

  fn init(
    (track, source): Self::Init,
    root: Self::Root,
    sender: ComponentSender<Self>,
  ) -> ComponentParts<Self> {
    let lyrics = match source {
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

    let lyrics_lines = build_lyrics_lines(&lyrics);

    let model = ViewLyricsModel {
      track: *track,
      lyrics_lines,
    };

    let lyrics_lines_box = model.lyrics_lines.widget();

    let widgets = view_output!();

    // Handle key presses
    let sender_handle = sender.clone();
    let controller = EventControllerKey::new();
    controller.connect_key_pressed(move |_con, key, _idx, modifier| {
      trace!("ViewLyrics key event: key {key} + {:?}", modifier);
      if key == gtk::gdk::Key::Escape {
        sender_handle
          .output(ViewLyricsOutput::Close)
          .expect("ViewLyricsOutput receiver dropped");
      }
      gtk::glib::Propagation::Proceed
    });
    root.add_controller(controller);

    ComponentParts { model, widgets }
  }
}

fn build_lyrics_lines(lyrics: &str) -> FactoryVecDeque<ViewLyricsLine> {
  let lines = LyricsLine::from_lyrics(lyrics);
  let mut view_lyrics_lines = FactoryVecDeque::builder()
    .launch(gtk::Box::new(gtk::Orientation::Vertical, 0))
    .detach();

  {
    let mut guard = view_lyrics_lines.guard();
    for line in lines {
      guard.push_back(line);
    }
  }

  view_lyrics_lines
}
