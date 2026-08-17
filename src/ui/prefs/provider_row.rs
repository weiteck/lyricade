use adw::prelude::*;
use relm4::{
  actions::{RelmAction, RelmActionGroup},
  gtk::gio::Menu,
  prelude::*,
};
use tracing::trace;

use crate::provider::{ProviderId, ProviderTier};

pub(super) struct ProviderRow {
  pub(crate) index: DynamicIndex,
  pub(crate) name: String,
  pub(crate) state: ProviderRowState,

  menu: Menu,
  action_move_up: RelmAction<ActionMoveUp>,
  action_move_down: RelmAction<ActionMoveDown>,
  action_toggle: RelmAction<ActionMoveToggle>,
  action_swap_tier: RelmAction<ActionMoveSwapTier>,
}

#[derive(Debug)]
pub(super) enum ProviderRowMsg {
  ListChangedWithLength(usize),
  MoveUp,
  MoveDown,
  SwapTier,
  Toggle,
  Enable(bool),
}

#[derive(Debug)]
pub(super) enum ProviderRowOutput {
  MoveUp(ProviderRowState),
  MoveDown(ProviderRowState),
  SwapTier(ProviderRowState),
  Toggle(ProviderRowState),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProviderRowState {
  pub(crate) id: ProviderId,
  pub(crate) enabled: bool,
  pub(crate) tier: ProviderTier,
}

relm4::new_action_group!(ProviderRowActionGroup, "provider_row_menu");
relm4::new_stateless_action!(ActionMoveUp, ProviderRowActionGroup, "move_up");
relm4::new_stateless_action!(ActionMoveDown, ProviderRowActionGroup, "move_down");
relm4::new_stateless_action!(ActionMoveSwapTier, ProviderRowActionGroup, "swap_tier");
relm4::new_stateless_action!(ActionMoveToggle, ProviderRowActionGroup, "toggle");

#[relm4::factory(pub)]
impl FactoryComponent for ProviderRow {
  type Init = ProviderRowState;
  type Input = ProviderRowMsg;
  type Output = ProviderRowOutput;
  type CommandOutput = ();
  type ParentWidget = gtk::ListBox;

  view! {
    #[name = "row_widget"]
    adw::ActionRow {
      set_use_markup: false,
      set_title: &self.name,
      set_class_active: ("dimmed", !self.state.enabled),
      set_activatable: false,

      add_prefix = &gtk::Box {
        gtk::Image {
          set_icon_name: Some("list-drag-handle-symbolic"),
          set_css_classes: &["dimmed", "drag-handle"],
        },
      },

      add_suffix = &gtk::Box {
        gtk::MenuButton {
          set_icon_name: "view-more-symbolic",
          set_direction: gtk::ArrowType::Down,
          set_valign: gtk::Align::Center,
          set_vexpand: false,
          set_menu_model: Some(menu),
          set_css_classes: &["flat", "popup"],
        }
      },
    },
  }

  fn init_model(state: Self::Init, index: &Self::Index, sender: FactorySender<Self>) -> Self {
    let sender_handle = sender.clone();
    let action_move_up: RelmAction<ActionMoveUp> = RelmAction::new_stateless(move |_| {
      sender_handle.input(ProviderRowMsg::MoveUp);
    });

    let sender_handle = sender.clone();
    let action_move_down: RelmAction<ActionMoveDown> = RelmAction::new_stateless(move |_| {
      sender_handle.input(ProviderRowMsg::MoveDown);
    });

    let sender_handle = sender.clone();
    let action_toggle: RelmAction<ActionMoveToggle> = RelmAction::new_stateless(move |_| {
      sender_handle.input(ProviderRowMsg::Toggle);
    });

    let sender_handle = sender.clone();
    let action_swap_tier: RelmAction<ActionMoveSwapTier> = RelmAction::new_stateless(move |_| {
      sender_handle.input(ProviderRowMsg::SwapTier);
    });

    let menu = gtk::gio::Menu::new();
    menu.append(Some("Move _Up"), Some("provider_row_menu.move_up"));
    menu.append(Some("Move _Down"), Some("provider_row_menu.move_down"));

    let section = gtk::gio::Menu::new();
    let swap_text = match state.tier {
      ProviderTier::Primary => "Move to _Fallback",
      ProviderTier::Secondary => "Move to _Primary",
    };
    section.append(Some(swap_text), Some("provider_row_menu.swap_tier"));
    menu.append_section(None, &section);

    let section = gtk::gio::Menu::new();
    section.append(
      Some(if state.enabled { "Disabl_e" } else { "_Enable" }),
      Some("provider_row_menu.toggle"),
    );
    menu.append_section(None, &section);

    Self {
      index: index.clone(),
      name: state.id.to_string(),
      state,
      menu,
      action_move_up,
      action_move_down,
      action_toggle,
      action_swap_tier,
    }
  }

  fn init_widgets(
    &mut self,
    index: &Self::Index,
    root: Self::Root,
    _parent: &<Self::ParentWidget as relm4::factory::FactoryView>::ReturnedWidget,
    _sender: FactorySender<Self>,
  ) -> Self::Widgets {
    trace!("Building ProviderRow for {} at index {}", &self.state.id, &index.current_index());

    let mut actions_group = RelmActionGroup::<ProviderRowActionGroup>::new();
    actions_group.add_action(self.action_move_up.clone());
    actions_group.add_action(self.action_move_down.clone());
    actions_group.add_action(self.action_toggle.clone());
    actions_group.add_action(self.action_swap_tier.clone());
    actions_group.register_for_widget(&root);

    let menu = &self.menu;

    let drag_source = gtk::DragSource::default();
    let name = self.name.clone();
    drag_source.set_actions(gtk::gdk::DragAction::MOVE);
    drag_source.connect_prepare(move |_ds, _x, _y| {
      let name = name.clone();
      let value = gtk::glib::Value::from(name);
      Some(gtk::gdk::ContentProvider::for_value(&value))
    });

    let widgets = view_output!();
    widgets.row_widget.add_controller(drag_source);

    widgets
  }

  fn update_with_view(
    &mut self,
    widgets: &mut Self::Widgets,
    message: Self::Input,
    sender: FactorySender<Self>,
  ) {
    match message {
      ProviderRowMsg::ListChangedWithLength(len) => {
        let idx = self.index.current_index();

        self.action_move_up.set_enabled(idx != 0);
        self
          .action_move_down
          .set_enabled(idx != len.saturating_sub(1));

        // Must have at least one primary Provider
        self
          .action_swap_tier
          .set_enabled(self.state.tier != ProviderTier::Primary || len > 1);
        self
          .action_toggle
          .set_enabled(self.state.tier != ProviderTier::Primary || len > 1);
      }

      ProviderRowMsg::MoveUp => {
        sender
          .output(ProviderRowOutput::MoveUp(self.state))
          .expect("ProviderRowOut receiver dropped");
      }

      ProviderRowMsg::MoveDown => {
        sender
          .output(ProviderRowOutput::MoveDown(self.state))
          .expect("ProviderRowOut receiver dropped");
      }

      ProviderRowMsg::SwapTier => {
        sender
          .output(ProviderRowOutput::SwapTier(self.state))
          .expect("ProviderRowOut receiver dropped");
      }

      ProviderRowMsg::Toggle => {
        sender
          .output(ProviderRowOutput::Toggle(self.state))
          .expect("ProviderRowOut receiver dropped");
      }

      ProviderRowMsg::Enable(active) => {
        if self.state.enabled == active {
          return;
        }

        self.state.enabled = active;

        let section = gtk::gio::Menu::new();
        section.append(
          Some(if self.state.enabled {
            widgets.row_widget.remove_css_class("dimmed");
            "Disable"
          } else {
            widgets.row_widget.add_css_class("dimmed");
            "Enable"
          }),
          Some("provider_row_menu.toggle"),
        );

        self.menu.remove(3);
        self.menu.append_section(None, &section);
      }
    }
  }
}
