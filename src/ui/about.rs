use relm4::prelude::*;

use crate::settings::{APP_NAME, APP_NAME_PRETTY};

pub(crate) struct AboutModel;

#[relm4::component(pub)]
impl SimpleComponent for AboutModel {
  type Input = ();
  type Output = ();
  type Init = ();

  view! {
    adw::AboutDialog {
      set_application_name: APP_NAME_PRETTY,
      set_application_icon: "lyricade",
      set_copyright: "Copyright © 2026 Chris Price",
      set_developer_name: "Chris Price",
      set_developers: &["Chris Price <fair.lake5766@fastmail.com>"],
      set_designers: &["Chris Price <fair.lake5766@fastmail.com>"],
      set_license_type: gtk::License::Apache20,
      set_website: &format!("https://github.com/weiteck/{APP_NAME}"),
      set_issue_url: &format!("https://github.com/weiteck/{APP_NAME}/issues"),
      set_comments: r"Thanks go to <b>tranxuanthang</b> and the <b>lrclib.net</b> contributors for creating the service that this app relies upon.

    <i>github.com/tranxuanthang/lrclib</i>",
      set_version: env!("CARGO_PKG_VERSION"),
      set_release_notes_version: env!("CARGO_PKG_VERSION"),
      set_release_notes: r"<p>This release adds to the lyrics management features.</p>
    <ul>
      <li>Add conditional guards for deleting sidecar files if a tag exists</li>
      <li>Add conditional guards for deleting lyrics tags if a sidecar file exists</li>
    </ul>",
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
