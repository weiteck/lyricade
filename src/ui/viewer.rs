use adw::prelude::*;
use relm4::{gtk::EventControllerKey, prelude::*};
use tracing::{debug, trace};

use crate::{
  lyrics::lyrics_line::LyricsLine,
  track::Track,
  ui::viewer::{line::ViewLyricsLine, tag::ViewLyricsLrcTag},
};

pub mod line;
pub mod tag;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewLyricsSource {
  Tag,
  Lrc,
  Txt,
}

pub struct ViewLyricsModel {
  track: Track,
  lyrics: String,
  lyrics_lines: FactoryVecDeque<ViewLyricsLine>,
  lrc_tags: FactoryVecDeque<ViewLyricsLrcTag>,
  is_viewing_raw: bool,
}

#[derive(Debug)]
pub enum ViewLyricsMsg {
  SetViewingRaw(bool),
}

#[derive(Debug)]
pub enum ViewLyricsOutput {
  Close,
}

#[relm4::component(pub)]
impl SimpleComponent for ViewLyricsModel {
  type Input = ViewLyricsMsg;
  type Output = ViewLyricsOutput;
  type Init = (Box<Track>, ViewLyricsSource);

  view! {
    #[root]
    adw::Window {
      set_title: Some("Lyrics"),
      set_default_size: (600, 700),

      #[wrap(Some)]
      set_content = &adw::ToolbarView {
        add_top_bar = &adw::HeaderBar {
          pack_start = &gtk::ToggleButton {
            set_icon_name: "format-text-rich-symbolic",
            add_css_class: "flat",
            set_tooltip: "View Raw Text",
            #[watch]
            set_active: model.is_viewing_raw,

            connect_toggled[sender] => move |btn| {
              sender.input(ViewLyricsMsg::SetViewingRaw(btn.is_active()));
            },
          },
        },

        set_content: Some(&view_stack),
      },

    },

    #[name = "view_stack"]
    // Stylised lyrics page
    adw::ViewStack {
      add = &gtk::ScrolledWindow {
        gtk::Box {
          set_orientation: gtk::Orientation::Vertical,
          set_spacing: 12,

          gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_margin_horizontal: 24,
            set_margin_top: 24,
            set_css_classes: &["view-lyrics", "track-info"],

            gtk::Label {
              set_halign: gtk::Align::Start,
              set_label: &model.track.artist_name,
              set_css_classes: &["view-lyrics", "artist-name"],
              set_hexpand: true,
              set_vexpand: false,
              set_halign: gtk::Align::Fill,
              set_xalign: 0.0,
              set_wrap: true,
              set_wrap_mode: gtk::pango::WrapMode::WordChar,
              set_margin_bottom: 6,
            },

            gtk::Label {
              set_label: &model.track.track_name,
              set_css_classes: &["view-lyrics", "track-name"],
              set_hexpand: true,
              set_vexpand: false,
              set_halign: gtk::Align::Fill,
              set_xalign: 0.0,
              set_wrap: true,
              set_wrap_mode: gtk::pango::WrapMode::WordChar,
            },
          },

          // LRC tags (if any)
          gtk::ScrolledWindow {
            set_visible: !model.lrc_tags.is_empty(),
            set_hscrollbar_policy: gtk::PolicyType::External,
            set_vscrollbar_policy: gtk::PolicyType::Never,
            set_hexpand: true,
            set_margin_top: 12,

            #[local_ref]
            lrc_tags_box -> gtk::Box {
              set_orientation: gtk::Orientation::Horizontal,
              set_halign: gtk::Align::Center,
              set_margin_horizontal: 24,
              set_spacing: 6,
            },
          },

          // Lyrics
          #[local_ref]
          lyrics_lines_box -> gtk::Box {
            set_css_classes: &["view-lyrics", "container"],
            set_halign: gtk::Align::Fill,
            set_valign: gtk::Align::Start,
            set_expand: true,
            set_margin_all: 24,
          },
        },
      } -> {
        // returned `ViewStackPage`
        set_title: Some("Stylised"),
        set_name: Some("stylised"),
        set_icon_name: Some("magic-wand-symbolic"),
      },

      // Raw lyrics page
      add = &gtk::ScrolledWindow {
        gtk::Box {
          set_halign: gtk::Align::Fill,
          set_valign: gtk::Align::Start,
          set_expand: true,
          set_margin_all: 24,

          gtk::Label {
            set_label: &model.lyrics,
          },
        }
      } -> {
        // returned `ViewStackPage`
        set_title: Some("Raw"),
        set_name: Some("raw"),
        set_icon_name: Some("format-text-rich-symbolic"),
      },

      #[watch]
      set_visible_child_name: if model.is_viewing_raw { "raw" } else { "stylised" },
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
        .unwrap_or_else(|| "No lyrics tag found".into()),
      ViewLyricsSource::Lrc => track
        .lyrics_sidecar_lrc_file
        .clone()
        .unwrap_or_else(|| "No LRC sidecar file found".into()),
      ViewLyricsSource::Txt => track
        .lyrics_sidecar_txt_file
        .clone()
        .unwrap_or_else(|| "No TXT sidecar file found".into()),
    };

    let (lyrics_lines, lrc_tags) = build_factories(&lyrics);

    let model = ViewLyricsModel {
      track: *track,
      lyrics,
      lyrics_lines,
      lrc_tags,
      is_viewing_raw: false,
    };

    let lyrics_lines_box = model.lyrics_lines.widget();
    let lrc_tags_box = model.lrc_tags.widget();

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

  fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
    match message {
      ViewLyricsMsg::SetViewingRaw(enabled) => {
        debug!("ViewLyrics: Viewing raw text page: {}", enabled);
        self.is_viewing_raw = enabled;
      }
    }
  }
}

fn build_factories(
  lyrics: &str,
) -> (FactoryVecDeque<ViewLyricsLine>, FactoryVecDeque<ViewLyricsLrcTag>) {
  let (lines, tags) = LyricsLine::from_lyrics(lyrics);

  let mut lyrics_lines = FactoryVecDeque::builder()
    .launch(gtk::Box::new(gtk::Orientation::Vertical, 0))
    .detach();
  {
    let mut guard = lyrics_lines.guard();
    for line in lines {
      guard.push_back(line);
    }
  }

  let mut lrc_tags = FactoryVecDeque::builder()
    .launch(gtk::Box::new(gtk::Orientation::Vertical, 0))
    .detach();
  {
    if let Some(tags) = tags {
      let mut guard = lrc_tags.guard();
      for tag in tags {
        guard.push_back(tag);
      }
    }
  }

  (lyrics_lines, lrc_tags)
}
