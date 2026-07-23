use adw::prelude::*;
use relm4::prelude::*;
use tracing::trace;

use crate::{lyrics::lyrics_line::LyricsLine, util};

const MIN_SPACING: i32 = 24;
const MAX_SPACING: i32 = 96;

pub(super) struct ViewLyricsLine {
  pub(super) inner: LyricsLine,
  pub(super) index: usize,
  pub(super) dimmed: bool,
}

#[derive(Debug)]
pub(super) enum LyricsLineMsg {
  SetDimmed(bool),
}

#[relm4::factory(pub)]
impl FactoryComponent for ViewLyricsLine {
  type Init = LyricsLine;
  type Input = LyricsLineMsg;
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
          set_wrap: false,
          set_css_classes: &["view-lyrics", "timestamp", "caption"],
          set_label: self.inner.timestamp.as_deref().unwrap_or(""),
        },
      },

      gtk::Label {
        #[watch]
        set_class_active: ("dimmed", self.dimmed),
        set_css_classes: &["view-lyrics", "text", "document"],
        set_hexpand: true,
        set_vexpand: false,
        set_valign: gtk::Align::Center,
        set_halign: gtk::Align::Fill,
        set_xalign: 0.0,
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
      dimmed: false,
    }
  }

  fn update(&mut self, message: Self::Input, _sender: FactorySender<Self>) {
    match message {
      LyricsLineMsg::SetDimmed(dimmed) => {
        trace!("ViewLyricsLine: {} set as dimmed: {dimmed}", self.index);

        self.dimmed = dimmed;
      }
    }
  }
}
