use relm4::{
  gtk::{EventControllerKey, prelude::WidgetExt},
  prelude::*,
};
use tracing::trace;

use crate::settings::APP_NAME_PRETTY;

pub struct AboutModel;

#[derive(Debug)]
pub enum AboutOutput {
  Close,
}

#[relm4::component(pub)]
impl SimpleComponent for AboutModel {
  type Input = ();
  type Output = AboutOutput;
  type Init = ();

  view! {
    adw::AboutWindow {
      set_application_name: APP_NAME_PRETTY,
      set_application_icon: "lyricade",
      set_developer_name: "Chris Price",
      set_developers: &["Chris Price <fair.lake5766@fastmail.com>"],
      set_version: env!("CARGO_PKG_VERSION"),
      set_license_type: gtk::License::MitX11,
      set_copyright: "Copyright © 2026 Chris Price",
      set_comments: r"Thanks go to <b>tranxuanthang</b> and the <b>lrclib.net</b> contributors for creating the service that this app relies upon.

    <i>github.com/tranxuanthang/lrclib</i>",
      // TODO: Change website
      set_website: "https://github.com/weiteck/lrcman",
    }
  }

  fn init(
    _init: Self::Init,
    root: Self::Root,
    sender: ComponentSender<Self>,
  ) -> ComponentParts<Self> {
    let model = AboutModel;
    let widgets = view_output!();

    // Handle key presses
    let sender_handle = sender.clone();
    let controller = EventControllerKey::new();
    controller.connect_key_pressed(move |_con, key, _idx, modifier| {
      trace!("About key event: key {key} + {:?}", modifier);
      if key == gtk::gdk::Key::Escape {
        sender_handle
          .output(AboutOutput::Close)
          .expect("AboutOutput receiver dropped");
      }
      gtk::glib::Propagation::Proceed
    });
    root.add_controller(controller);

    ComponentParts { model, widgets }
  }
}
