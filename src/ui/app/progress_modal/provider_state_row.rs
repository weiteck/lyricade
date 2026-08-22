use std::sync::{Arc, atomic::Ordering};

use adw::prelude::*;
use num_format::ToFormattedString;
use relm4::prelude::*;
use tracing::trace;

use crate::{NUM_LOCALE, provider::ProviderState};

const VALUES_SPACING: i32 = 6;
const VALUE_MARGIN: i32 = 4;

pub(super) struct ProviderStateRow {
  pub(crate) state: Arc<ProviderState>,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ProviderStateRowMsg {
  Tick,
}

#[relm4::factory(pub)]
impl FactoryComponent for ProviderStateRow {
  type Init = Arc<ProviderState>;
  type Input = ProviderStateRowMsg;
  type Output = ();
  type CommandOutput = ();
  type ParentWidget = gtk::Box;

  view! {
    #[name = "row_widget"]
    gtk::Box {
      set_can_focus: false,
      set_focusable: false,

      // Provider header
      gtk::Box {
        set_halign: gtk::Align::Center,
        set_hexpand: true,

        gtk::Label {
          add_css_class: "heading",
          set_ellipsize: gtk::pango::EllipsizeMode::End,
          set_text: &self.state.id.to_string(),
          set_tooltip: &format!("Lyrics Provider: {}", self.state.id),
        },
      },

      // Values
      gtk::Box {
        set_halign: gtk::Align::End,
        set_hexpand: true,
        set_homogeneous: true,
        set_spacing: VALUES_SPACING,

        // Value 1
        gtk::Box {
          set_orientation: gtk::Orientation::Vertical,
          add_css_class: "card",

          gtk::Box {
            set_halign: gtk::Align::Center,
            set_margin_all: VALUE_MARGIN,
            gtk::Label {
              add_css_class: "title",
              set_width_chars: 3,
              #[watch]
              set_text: &self.state.current_requests.load(Ordering::Relaxed).to_formatted_string(&*NUM_LOCALE),
              set_tooltip: "Current Requests",
            },
          },
        },

        // Value 2
        gtk::Box {
          set_orientation: gtk::Orientation::Vertical,
          add_css_class: "card",

          gtk::Box {
            set_halign: gtk::Align::Center,
            set_margin_all: VALUE_MARGIN,
            gtk::Label {
              set_width_chars: 3,
              add_css_class: "title",
              #[watch]
              set_text: &self.state.total_requests.load(Ordering::Relaxed).to_formatted_string(&*NUM_LOCALE),
              set_tooltip: "Total Requests",
            },
          },
        },

        // Value 3
        gtk::Box {
          set_orientation: gtk::Orientation::Vertical,
          add_css_class: "card",

          gtk::Box {
            set_halign: gtk::Align::Center,
            set_margin_all: VALUE_MARGIN,
            gtk::Image {
              #[watch]
              set_opacity: if self.state.rate_limited.load(Ordering::Relaxed) { 1.0 } else { 0.15 },
              set_icon_name: Some("media-playback-pause-symbolic"),
              set_tooltip: "Rate-Limited",
            },
          },
        },
      },
    },
  }

  fn init_model(state: Self::Init, index: &Self::Index, _sender: FactorySender<Self>) -> Self {
    trace!("Building ProviderStateRow for {} at index {}", &state.id, &index.current_index());

    Self { state }
  }

  fn update(&mut self, message: Self::Input, _sender: FactorySender<Self>) {
    match message {
      ProviderStateRowMsg::Tick => {}
    }
  }
}
