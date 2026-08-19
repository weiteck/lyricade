use std::{cell::Cell, rc::Rc};

use adw::prelude::*;
use relm4::{adw::AlertDialog, prelude::*};
use tracing::{debug, error, trace};

use crate::{
  PROVIDER_MANAGER,
  ui::app::{
    ProgressUpdate,
    progress_modal::provider_state_row::{ProviderStateRow, ProviderStateRowMsg},
  },
};

mod provider_state_row;

pub(crate) struct ProgressModalModel {
  alert_dialog: AlertDialog,
  progress: ProgressUpdate,
  provider_state_rows: FactoryVecDeque<ProviderStateRow>,
  show_provider_state: bool,
  showing: Rc<Cell<bool>>,
  closing_programmatically: Rc<Cell<bool>>,
}

#[derive(Debug)]
pub(crate) struct ProgressModalInit {
  pub(crate) progress: ProgressUpdate,
  pub(crate) show_provider_state: bool,
  pub(crate) heading: Option<String>,
  pub(crate) body: Option<String>,
}

#[bon::bon]
impl ProgressModalInit {
  #[builder]
  pub(crate) fn new(
    progress: ProgressUpdate,
    show_provider_state: bool,
    heading: Option<String>,
    body: Option<String>,
  ) -> Self {
    Self {
      progress,
      show_provider_state,
      heading,
      body,
    }
  }
}

#[derive(Debug)]
pub(crate) enum ProgressModalMsg {
  Show(ProgressModalInit),
  Hide,
  UpdateState(ProgressUpdate),
  RefreshProviders,
}

#[derive(Debug)]
pub(crate) enum ProgressModalOutput {
  Cancel,
}

#[relm4::component(pub)]
impl Component for ProgressModalModel {
  type Input = ProgressModalMsg;
  type Output = ProgressModalOutput;
  type CommandOutput = ();
  type Init = adw::ApplicationWindow;

  view! {
    gtk::Box {
      set_expand: true,
      set_align: gtk::Align::Fill,
      set_orientation: gtk::Orientation::Vertical,
      set_margin_all: 12,
      set_spacing: 36,

      gtk::Box {
        set_orientation: gtk::Orientation::Vertical,
        set_spacing: 12,

        gtk::Label {
          #[watch]
          set_text: model.progress.step.as_ref().map_or("", |s| s.as_str()),
        },

        #[name = "pb"]
        gtk::ProgressBar {
          set_expand: true,
          set_halign: gtk::Align::Fill,
          set_valign: gtk::Align::Center,
          #[watch]
          set_fraction: model.progress.progress,
        },
      },

      gtk::Box {
        #[watch]
        set_visible: model.show_provider_state,
        #[local_ref]
        provider_state_rows_box -> gtk::Box {
          set_orientation: gtk::Orientation::Vertical,
          set_spacing: 12,
          set_homogeneous: true,
        },
      },
    },
  }

  fn init(
    _init: Self::Init,
    root: Self::Root,
    sender: ComponentSender<Self>,
  ) -> ComponentParts<Self> {
    // Get Lyrics progress window Provider state rows
    let provider_state = PROVIDER_MANAGER.provider_state();
    let mut provider_state_rows = FactoryVecDeque::builder().launch_default().detach();
    {
      let mut guard = provider_state_rows.guard();
      provider_state.iter().cloned().for_each(|state| {
        guard.push_back(state);
      });
    }

    let alert_dialog = adw::AlertDialog::builder()
      .extra_child(&root)
      .default_response("cancel")
      .build();
    alert_dialog.add_response("cancel", "Cancel");

    let closing_programmatically = Rc::new(Cell::new(false));
    let showing = Rc::new(Cell::new(false));

    let sender_handle = sender.clone();
    let closing_programmatically_clone = Rc::clone(&closing_programmatically);
    let showing_clone = Rc::clone(&showing);
    alert_dialog.connect_response(None, move |_, _resp| {
      if closing_programmatically_clone.get() {
        trace!("Close requested programmatically");
      } else {
        debug!("User cancelled process");
        let _ = sender_handle
          .output(ProgressModalOutput::Cancel)
          .inspect_err(|_| error!("ProgressModalOutput receiver dropped"));
      }

      showing_clone.set(false);
    });

    let model = ProgressModalModel {
      alert_dialog,
      progress: ProgressUpdate::default(),
      provider_state_rows,
      show_provider_state: true,
      showing,
      closing_programmatically,
    };

    let provider_state_rows_box = model.provider_state_rows.widget();

    let widgets = view_output!();

    ComponentParts { model, widgets }
  }

  fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
    match message {
      ProgressModalMsg::Show(init) => {
        self.alert_dialog.set_heading(init.heading.as_deref());
        self
          .alert_dialog
          .set_body(init.body.as_deref().unwrap_or_default());
        self.show_provider_state = init.show_provider_state;

        sender.input(ProgressModalMsg::UpdateState(init.progress));

        self.showing.set(true);
        self.alert_dialog.present(None::<&adw::ApplicationWindow>);
      }

      ProgressModalMsg::Hide => {
        if self.showing.get() {
          self.closing_programmatically.set(true);
          self.alert_dialog.close();
          self.closing_programmatically.set(false);
        }
      }

      ProgressModalMsg::UpdateState(pu) => {
        self.progress = pu;

        // Ensure Provider state updates
        if self.show_provider_state {
          self
            .provider_state_rows
            .broadcast(ProviderStateRowMsg::Tick);
        }
      }

      ProgressModalMsg::RefreshProviders => {
        let provider_state = PROVIDER_MANAGER.provider_state();
        let mut guard = self.provider_state_rows.guard();
        guard.clear();
        provider_state.iter().cloned().for_each(|state| {
          guard.push_back(state);
        });
      }
    }
  }
}
