use std::sync::{Arc, atomic::Ordering};

use adw::prelude::*;
use relm4::{adw::ActionRow, gtk::ListBoxRow, prelude::*};
use tracing::trace;

use crate::provider::ProviderState;

pub(super) struct ProviderStateRow {
  pub(crate) state: Arc<ProviderState>,
}

#[derive(Debug)]
pub(super) enum ProviderStateRowMsg {
  UpdateState,
}

#[relm4::factory(pub)]
impl FactoryComponent for ProviderStateRow {
  type Init = Arc<ProviderState>;
  type Input = ProviderStateRowMsg;
  type Output = ();
  type CommandOutput = ();
  type ParentWidget = adw::ExpanderRow;

  view! {
    #[name = "row_widget"]
    gtk::ListBoxRow {
      set_selectable: false,
      set_activatable: false,

      gtk::Box {
        // set_halign: gtk::Align::Fill,
        // set_hexpand: true,
        set_homogeneous: true,
        set_spacing: 6,
        set_margin_all: 12,

        gtk::Box {
          set_halign: gtk::Align::Center,
          add_css_class: "heading",
          gtk::Label {
            set_text: &self.state.id.to_string(),
            set_tooltip: "Provider Name",
          },
        },

        gtk::Box {
          set_halign: gtk::Align::Center,
          gtk::Image {
            #[watch]
            set_visible: self.state.rate_limited.load(Ordering::Relaxed),
            set_icon_name: Some("media-playback-pause-symbolic"),
            set_tooltip: "Rate-Limited",
          },
        },

        gtk::Box {
          set_halign: gtk::Align::Center,
          gtk::Label {
            #[watch]
            set_text: &self.state.current_requests.load(Ordering::Relaxed).to_string(),
            set_tooltip: "Current Requests",
          },
        },

        gtk::Box {
          set_halign: gtk::Align::Center,
          gtk::Label {
            #[watch]
            set_text: &self.state.total_requests.load(Ordering::Relaxed).to_string(),
            set_tooltip: "Total Requests",
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
      ProviderStateRowMsg::UpdateState => {}
    }
  }
}
