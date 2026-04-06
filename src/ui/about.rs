use relm4::prelude::*;

use crate::settings::APP_NAME_PRETTY;

pub struct AboutModel;

#[relm4::component(pub)]
impl SimpleComponent for AboutModel {
    type Input = ();
    type Output = ();
    type Init = ();

    view! {
      adw::AboutWindow {
        // TODO: Change name
        set_application_name: APP_NAME_PRETTY,
        set_developer_name: "Chris Price",
        set_developers: &["Chris Price <fair.lake5766@fastmail.com>"],
        set_version: env!("CARGO_PKG_VERSION"),
        set_license_type: gtk::License::MitX11,
        set_comments: r"Thanks go to <b>tranxuanthang</b> and the <b>lrclib.net</b> contributors for creating the service that this app relies upon.

    <i>github.com/tranxuanthang/lrclib</i>",
        // TODO: Change website
        set_website: "https://github.com/weiteck/lrcman",
      }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = AboutModel;
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }
}
