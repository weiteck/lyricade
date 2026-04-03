use relm4::{gtk::prelude::GtkWindowExt, prelude::*};

pub struct SettingsModel;

#[relm4::component(pub)]
impl SimpleComponent for SettingsModel {
    type Input = ();
    type Output = ();
    type Init = ();

    view! {
      adw::Window {
        set_title: Some("Settings"),

        adw::ToolbarView {
          add_top_bar = &adw::HeaderBar {
          },
        },
      },
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = SettingsModel;
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }
}
