use relm4::{
  gtk::{EventControllerKey, gdk, prelude::WidgetExt},
  prelude::*,
};
use tracing::trace;

use crate::settings::{APP_NAME, APP_NAME_PRETTY};

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
      set_license_type: gtk::License::Apache20,
      set_license: include_str!("../../LICENSE"),
      set_website: &format!("https://github.com/weiteck/{APP_NAME}"),
      set_version: env!("CARGO_PKG_VERSION"),
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
      if key == gdk::Key::Escape
        || (key.to_upper() == gdk::Key::W && modifier.contains(gdk::ModifierType::CONTROL_MASK))
      {
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
