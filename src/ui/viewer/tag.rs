use adw::prelude::*;
use relm4::prelude::*;

use crate::lyrics::lrc::LrcTag;

pub(super) struct ViewLyricsLrcTag {
  pub inner: LrcTag,
}

#[relm4::factory(pub)]
impl FactoryComponent for ViewLyricsLrcTag {
  type Init = LrcTag;
  type Input = ();
  type Output = ();
  type CommandOutput = ();
  type ParentWidget = gtk::Box;

  view! {
    gtk::Box {
      set_css_classes: &["view-lyrics", "tag"],
      set_tooltip: &self.inner.to_string(),
      set_spacing: 9,

      gtk::Label {
        set_css_classes: &["view-lyrics", "tag", "type"],
        set_label: &self.inner.tag(),
      },

      gtk::Label {
        set_css_classes: &["view-lyrics", "tag", "value"],
        set_label: &self.inner.value(),
      },
    },
  }

  fn init_model(lrc_tag: Self::Init, _index: &Self::Index, _sender: FactorySender<Self>) -> Self {
    Self { inner: lrc_tag }
  }
}
