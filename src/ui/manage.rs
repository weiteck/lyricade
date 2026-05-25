use adw::prelude::*;
use relm4::{gtk::EventControllerKey, prelude::*};
use relm4_components::alert::{Alert, AlertMsg, AlertResponse, AlertSettings};
use tracing::{debug, trace};

use crate::manage::{ManageLyricsOptions, ManageLyricsTarget};

pub struct ManageLyricsModel {
  state: ManageLyricsOptions,
  default_state: ManageLyricsOptions,
  confirm_dialog: Controller<Alert>,
}

#[derive(Debug)]
pub enum ManageLyricsMsg {
  UpdateState(ExposedSetting),
  ShowConfirmDialog,
  Confirm,
  ResetState,
  Noop,
}

#[derive(Debug)]
pub enum ManageLyricsOutput {
  Close,
  Confirm(ManageLyricsOptions),
}

#[derive(Debug)]
pub enum ExposedSetting {
  TagsDelete(ManageLyricsTarget),
  TagsCopy(ManageLyricsTarget),
  TagsConvertToPlain(bool),
  SidecarsDelete(ManageLyricsTarget),
  SidecarsCopy(ManageLyricsTarget),
  SidecarsConvertToPlain(bool),
}

#[relm4::component(pub)]
impl SimpleComponent for ManageLyricsModel {
  type Input = ManageLyricsMsg;
  type Output = ManageLyricsOutput;
  type Init = ();

  view! {
    adw::Window {
      set_title: Some("Manage Lyrics"),
      set_default_width: 600,

      #[wrap(Some)]
      set_content = &adw::ToolbarView {
        add_top_bar = &adw::HeaderBar {},

        #[wrap(Some)]
        set_content = &adw::PreferencesPage {
          adw::PreferencesGroup {
            set_title: "Lyrics Tags",
            set_description: Some("What actions to take on lyrics tags embedded in your audio files."),

            gtk::ListBox {
              add_css_class: "boxed-list",

              adw::ComboRow {
                set_title: "_Delete Lyrics Tags",
                set_use_underline: true,
                set_subtitle: "Delete lyrics tags from audio files",
                set_model: Some(&gtk::StringList::new(&[
                  "Do Nothing",
                  "Delete Plain",
                  "Delete Sync",
                  "Delete All",
                ])),
                #[watch]
                set_selected: model.state.tags.delete as u32,
                connect_selected_item_notify[sender] => move |row| {
                  let target = ManageLyricsTarget::from(row.selected());
                  sender.input(ManageLyricsMsg::UpdateState(ExposedSetting::TagsDelete(target)));
                },
              },

              adw::ComboRow {
                set_title: "_Copy From Sidecar Files",
                set_use_underline: true,
                set_subtitle: "Copy to lyrics tags from LRC/TXT files saved alongside",
                set_model: Some(&gtk::StringList::new(&[
                  "Do Nothing",
                  "Copy Plain",
                  "Copy Sync",
                  "Copy Any",
                ])),
                #[watch]
                set_sensitive: model.state.tags.delete == ManageLyricsTarget::None,
                #[watch]
                set_selected: if model.state.tags.delete == ManageLyricsTarget::None { model.state.tags.copy as u32 } else { 0 },
                connect_selected_item_notify[sender] => move |row| {
                  let target = ManageLyricsTarget::from(row.selected());
                  sender.input(ManageLyricsMsg::UpdateState(ExposedSetting::TagsCopy(target)));
                },
              },

              adw::SwitchRow {
                set_title: "Con_vert Sync to Plain Lyrics",
                set_use_underline: true,
                #[watch]
                set_sensitive: model.state.tags.delete == ManageLyricsTarget::None,
                set_subtitle: "Convert synchronous LRC lyrics tags to plain TXT lyrics",
                #[watch]
                set_active: model.state.tags.convert_to_plain && model.state.tags.delete == ManageLyricsTarget::None,
                connect_active_notify[sender] => move |btn| {
                  sender.input(ManageLyricsMsg::UpdateState(ExposedSetting::TagsConvertToPlain(btn.is_active())));
                },
              },
            },
          },

          adw::PreferencesGroup {
            set_title: "Sidecar Files",
            set_description: Some("What actions to take on LRC/TXT lyrics files."),

            gtk::ListBox {
              add_css_class: "boxed-list",

              adw::ComboRow {
                set_title: "Delete _Sidecar Files",
                set_use_underline: true,
                set_subtitle: "Delete synchronous LRC or plain TXT sidecar lyrics files",
                set_model: Some(&gtk::StringList::new(&[
                  "Do Nothing",
                  "Delete Plain",
                  "Delete Sync",
                  "Delete All",
                ])),
                #[watch]
                set_selected: model.state.sidecars.delete as u32,
                connect_selected_item_notify[sender] => move |row| {
                  let target = ManageLyricsTarget::from(row.selected());
                  sender.input(ManageLyricsMsg::UpdateState(ExposedSetting::SidecarsDelete(target)));
                },
              },

              adw::ComboRow {
                set_title: "Copy From _Lyrics Tags",
                set_use_underline: true,
                set_subtitle: "Copy to LRC/TXT files from lyrics tags",
                set_model: Some(&gtk::StringList::new(&[
                  "Do Nothing",
                  "Copy Plain",
                  "Copy Sync",
                  "Copy Any",
                ])),
                #[watch]
                set_sensitive: model.state.sidecars.delete == ManageLyricsTarget::None,
                #[watch]
                set_selected: if model.state.sidecars.delete == ManageLyricsTarget::None { model.state.sidecars.copy as u32 } else { 0 },
                connect_selected_item_notify[sender] => move |row| {
                  let target = ManageLyricsTarget::from(row.selected());
                  sender.input(ManageLyricsMsg::UpdateState(ExposedSetting::SidecarsCopy(target)));
                },
              },

              adw::SwitchRow {
                set_title: "Conver_t Sync to Plain Lyrics",
                set_use_underline: true,
                set_subtitle: "Convert synchronous LRC lyrics files to plain TXT lyrics",
                #[watch]
                set_sensitive: model.state.sidecars.delete == ManageLyricsTarget::None,
                #[watch]
                set_active: model.state.sidecars.convert_to_plain && model.state.sidecars.delete == ManageLyricsTarget::None,
                connect_active_notify[sender] => move |btn| {
                  sender.input(ManageLyricsMsg::UpdateState(ExposedSetting::SidecarsConvertToPlain(btn.is_active())));
                },
              },
            },
          },

          adw::PreferencesGroup {
            gtk::Box {
              set_orientation: gtk::Orientation::Horizontal,
              set_halign: gtk::Align::Center,
              set_spacing: 12,

              gtk::Button {
                set_label: "Cancel",

                connect_clicked[sender] => move |_btn| {
                  sender.output(ManageLyricsOutput::Close)
                    .expect("ManageLyricsOutput receiver dropped");
                },
              },

              gtk::Button {
                set_label: "Apply",
                #[watch]
                set_sensitive: model.state != model.default_state,
                #[watch]
                set_class_active: ("destructive-action", model.state != model.default_state),

                connect_clicked[sender] => move |_btn| {
                  sender.input(ManageLyricsMsg::ShowConfirmDialog);
                },
              },
            },
          },
        },

      },

    },
  }

  fn init(
    _init: Self::Init,
    root: Self::Root,
    sender: ComponentSender<Self>,
  ) -> ComponentParts<Self> {
    let sender_handle = sender.clone();
    root.connect_show(move |_| sender_handle.input(ManageLyricsMsg::ResetState));

    let confirm_dialog = Alert::builder()
      .transient_for(&root)
      .launch(AlertSettings {
        text: Some("Are you sure?".into()),
        secondary_text: Some("This action cannot be undone.".into()),
        is_modal: true,
        destructive_accept: true,
        confirm_label: Some("Confirm".into()),
        cancel_label: Some("Cancel".into()),
        option_label: None,
        extra_child: None,
      })
      .forward(sender.input_sender(), |msg| match msg {
        AlertResponse::Confirm => ManageLyricsMsg::Confirm,
        AlertResponse::Cancel | AlertResponse::Option => ManageLyricsMsg::Noop,
      });

    let model = ManageLyricsModel {
      state: ManageLyricsOptions::default(),
      default_state: ManageLyricsOptions::default(),
      confirm_dialog,
    };

    let widgets = view_output!();

    // Handle key presses
    let sender_handle = sender.clone();
    let controller = EventControllerKey::new();
    controller.connect_key_pressed(move |_con, key, _idx, modifier| {
      trace!("ViewLyrics key event: key {key} + {:?}", modifier);
      if key == gtk::gdk::Key::Escape {
        sender_handle
          .output(ManageLyricsOutput::Close)
          .expect("ManageLyricsOutput receiver dropped");
      }
      gtk::glib::Propagation::Proceed
    });
    root.add_controller(controller);

    ComponentParts { model, widgets }
  }

  fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
    match message {
      ManageLyricsMsg::Noop => {
        trace!("ManageLyrics: No-op");
      }

      ManageLyricsMsg::UpdateState(setting) => {
        trace!("ManageLyrics: Updating state with setting: {:?}", &setting);

        match setting {
          ExposedSetting::TagsDelete(target) => self.state.tags.delete = target,
          ExposedSetting::TagsCopy(target) => self.state.tags.copy = target,
          ExposedSetting::TagsConvertToPlain(enabled) => self.state.tags.convert_to_plain = enabled,
          ExposedSetting::SidecarsDelete(target) => self.state.sidecars.delete = target,
          ExposedSetting::SidecarsCopy(target) => self.state.sidecars.copy = target,
          ExposedSetting::SidecarsConvertToPlain(enabled) => {
            self.state.sidecars.convert_to_plain = enabled;
          }
        }

        debug!("ManageLyrics: Updated state:\n{:#?}", &self.state);
      }

      ManageLyricsMsg::ShowConfirmDialog => {
        debug!("ManageLyrics: Showing confirmation dialog");
        self.confirm_dialog.emit(AlertMsg::Show);
      }

      ManageLyricsMsg::Confirm => {
        debug!("ManageLyrics: Confirmed");

        sender
          .output(ManageLyricsOutput::Confirm(self.state))
          .expect("ManageLyricsOutput receiver dropped");
      }

      ManageLyricsMsg::ResetState => {
        debug!("ManageLyrics: Resetting state");

        self.state = self.default_state;
      }
    }
  }
}
