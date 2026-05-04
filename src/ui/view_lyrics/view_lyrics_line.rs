use adw::prelude::*;
use relm4::prelude::*;

use crate::{lyrics::lyrics_line::LyricsLine, util};

const MIN_SPACING: i32 = 24;
const MAX_SPACING: i32 = 96;

pub(super) struct ViewLyricsLine {
  pub inner: LyricsLine,
  pub index: usize,
}

#[relm4::factory(pub)]
impl FactoryComponent for ViewLyricsLine {
  type Init = LyricsLine;
  type Input = ();
  type Output = ();
  type CommandOutput = ();
  type ParentWidget = gtk::Box;

  view! {
    gtk::Box {
      set_orientation: gtk::Orientation::Horizontal,
      set_hexpand: true,
      set_spacing: 24,
      // Set top margin based on time gap from last lyric line
      // unless this is the first lyric line
      set_margin_top: if self.index == 0 { 0 } else {
        util::scale()
        .value(self.inner.gap_to_prev.unwrap_or_default())
        .min(MIN_SPACING)
        .max(MAX_SPACING)
        .call()
      },

      gtk::Box {
        set_visible: self.inner.timestamp.is_some(),
        set_css_classes: &["view-lyrics", "timestamp", "dimmed"],
        set_valign: gtk::Align::Fill,
        set_vexpand: true,

        gtk::Label {
          set_valign: gtk::Align::Start,
          set_halign: gtk::Align::End,
          set_expand: false,
          set_css_classes: &["view-lyrics", "timestamp", "caption"],
          set_label: self.inner.timestamp.as_deref().unwrap_or(""),
        },
      },

      gtk::Label {
        set_css_classes: &["view-lyrics", "text", "document"],
        set_align: gtk::Align::Start,
        set_wrap: true,
        set_wrap_mode: gtk::pango::WrapMode::WordChar,
        set_label: &self.inner.contents,
      },
    },
  }

  fn init_model(
    lyrics_line: Self::Init,
    index: &Self::Index,
    _sender: FactorySender<Self>,
  ) -> Self {
    Self {
      inner: lyrics_line,
      index: index.current_index(),
    }
  }
}
