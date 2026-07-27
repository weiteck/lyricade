use adw::prelude::*;
use relm4::{gtk::EventControllerKey, prelude::*};
use tracing::{debug, trace};

use crate::manage::{ManageLyricsOptions, ManageLyricsTarget};

pub(crate) struct ManageLyricsModel {
  state: ManageLyricsOptions,
  default_state: ManageLyricsOptions,
  alert_dialog: adw::AlertDialog,
}

#[derive(Debug)]
pub(crate) enum ManageLyricsMsg {
  UpdateState(ExposedSetting),
  ShowConfirmDialog,
  Confirm,
  ResetState,
}

#[derive(Debug)]
pub(crate) enum ManageLyricsOutput {
  Close,
  Confirm(ManageLyricsOptions),
}

#[derive(Debug)]
pub(crate) enum ExposedSetting {
  TagsDelete(ManageLyricsTarget),
  TagsDeleteCondition(Option<ManageLyricsTarget>),
  TagsCopy(ManageLyricsTarget),
  TagsConvertToPlain(bool),
  SidecarsDelete(ManageLyricsTarget),
  SidecarsDeleteCondition(Option<ManageLyricsTarget>),
  SidecarsCopy(ManageLyricsTarget),
  SidecarsConvertToPlain(bool),
}

#[relm4::component(pub)]
impl Component for ManageLyricsModel {
  type Input = ManageLyricsMsg;
  type Output = ManageLyricsOutput;
  type CommandOutput = ();
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
                set_title: "C_onditional Delete",
                set_use_underline: true,
                set_subtitle: "Only delete tags if sidecar file exists",
                set_model: Some(&gtk::StringList::new(&[
                  "Unconditional",
                  "No Sidecar",
                  "Plain Sidecar",
                  "Sync Sidecar",
                  "Any Sidecar",
                ])),
                #[watch]
                set_sensitive: model.state.tags.delete != ManageLyricsTarget::None,
                #[watch]
                set_selected: if model.state.tags.delete == ManageLyricsTarget::None { 0 }
                  else if let Some(target) = model.state.tags.delete_on_sidecar_condition { target as u32 + 1}
                  else { 0 },

                  connect_selected_item_notify[sender] => move |row| {
                  let target = match row.selected() {
                    0 => None,
                    idx => Some(ManageLyricsTarget::from(idx - 1))
                  };
                  sender.input(ManageLyricsMsg::UpdateState(ExposedSetting::TagsDeleteCondition(target)));
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
                set_title: "Co_nditional Delete",
                set_use_underline: true,
                set_subtitle: "Only delete sidecar files if tag exists",
                set_model: Some(&gtk::StringList::new(&[
                  "Unconditional",
                  "No Tag",
                  "Plain Tag",
                  "Sync Tag",
                  "Any Tag",
                ])),
                #[watch]
                set_sensitive: model.state.sidecars.delete != ManageLyricsTarget::None,
                #[watch]
                set_selected: if model.state.sidecars.delete == ManageLyricsTarget::None { 0 }
                  else if let Some(target) = model.state.sidecars.delete_on_tag_condition { target as u32 + 1}
                  else { 0 },

                  connect_selected_item_notify[sender] => move |row| {
                  let target = match row.selected() {
                    0 => None,
                    idx => Some(ManageLyricsTarget::from(idx - 1))
                  };
                  sender.input(ManageLyricsMsg::UpdateState(ExposedSetting::SidecarsDeleteCondition(target)));
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
                add_css_class: "suggested-action",

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

    let alert_dialog = adw::AlertDialog::builder()
      .heading("Are you sure?")
      .body("This action cannot be undone.")
      .default_response("cancel")
      .close_response("cancel")
      .build();
    alert_dialog.add_response("cancel", "Cancel");
    alert_dialog.add_response("confirm", "Confirm");
    alert_dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);

    let sender_handle = sender.clone();
    alert_dialog.connect_response(None, move |_, resp| {
      if resp == "confirm" {
        sender_handle.input(ManageLyricsMsg::Confirm);
      }
    });

    let model = ManageLyricsModel {
      state: ManageLyricsOptions::default(),
      default_state: ManageLyricsOptions::default(),
      alert_dialog,
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

  fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
    match message {
      ManageLyricsMsg::UpdateState(setting) => {
        trace!("ManageLyrics: Updating state with setting: {:?}", &setting);

        match setting {
          ExposedSetting::TagsDelete(target) => self.state.tags.delete = target,
          ExposedSetting::TagsDeleteCondition(target) => {
            self.state.tags.delete_on_sidecar_condition = target;
          }
          ExposedSetting::TagsCopy(target) => self.state.tags.copy = target,
          ExposedSetting::TagsConvertToPlain(enabled) => self.state.tags.convert_to_plain = enabled,
          ExposedSetting::SidecarsDelete(target) => self.state.sidecars.delete = target,
          ExposedSetting::SidecarsDeleteCondition(target) => {
            self.state.sidecars.delete_on_tag_condition = target;
          }
          ExposedSetting::SidecarsCopy(target) => self.state.sidecars.copy = target,
          ExposedSetting::SidecarsConvertToPlain(enabled) => {
            self.state.sidecars.convert_to_plain = enabled;
          }
        }

        debug!("ManageLyrics: Updated state:\n{:#?}", &self.state);
      }

      ManageLyricsMsg::ShowConfirmDialog => {
        debug!("ManageLyrics: Showing confirmation dialog");
        self.alert_dialog.present(Some(root));
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
