use adw::prelude::*;
use relm4::{actions::*, prelude::*};

use crate::library::Library;

pub(super) struct LibraryRow {
  pub index: DynamicIndex,
  pub library: Library,
  pub name: String,
}

#[derive(Debug)]
pub(super) enum LibraryRowOutput {
  Delete(DynamicIndex),
  Edit(DynamicIndex),
}

#[relm4::factory(pub)]
impl FactoryComponent for LibraryRow {
  type Init = Library;
  type Input = ();
  type Output = LibraryRowOutput;
  type CommandOutput = ();
  type ParentWidget = gtk::ListBox;

  view! {
    adw::ActionRow {
      set_focusable: false,
      set_selectable: false,

      #[wrap(Some)]
      set_child = &gtk::Box {
        set_orientation: gtk::Orientation::Horizontal,
        set_hexpand: true,
        set_spacing: 12,
        set_margin_horizontal: 12,
        set_margin_vertical: 8,

        gtk::Box {
          set_orientation: gtk::Orientation::Vertical,
          set_halign: gtk::Align::Start,
          set_valign: gtk::Align::Center,
          set_hexpand: true,
          set_spacing: 6,

          gtk::Label {
            set_halign: gtk::Align::Start,
            set_ellipsize: gtk::pango::EllipsizeMode::End,
            set_label: &self.name,
            add_css_class: "title",
          },

          gtk::Label {
            set_halign: gtk::Align::Start,
            set_ellipsize: gtk::pango::EllipsizeMode::Middle,
            set_label: &self.library.path,
            set_tooltip: &self.library.path,
            add_css_class: "subtitle",
          },
        },

        gtk::MenuButton {
          add_css_class: "flat",
          set_menu_model: Some(&library_row_menu),
          set_icon_name: "view-more-symbolic",
        },
      },
    }
  }

  menu! {
    library_row_menu: {
      "Edit" => ActionEdit,
      "Delete" => ActionDelete,
    }
  }

  fn init_model(library: Self::Init, index: &Self::Index, _sender: FactorySender<Self>) -> Self {
    let name = library
      .name
      .as_ref()
      .cloned()
      .unwrap_or_else(|| library.default_name().to_string());

    Self {
      index: index.clone(),
      library,
      name,
    }
  }

  fn init_widgets(
    &mut self,
    _index: &Self::Index,
    root: Self::Root,
    _returned_widget: &<Self::ParentWidget as relm4::factory::FactoryView>::ReturnedWidget,
    sender: FactorySender<Self>,
  ) -> Self::Widgets {
    let widgets = view_output!();

    // Row menu actions
    relm4::new_action_group!(pub RowActionGroup, "library_row_action_group");
    let mut actions_group = RelmActionGroup::<RowActionGroup>::new();

    let index = self.index.clone();
    relm4::new_stateless_action!(ActionEdit, RowActionGroup, "edit");
    let action_edit: RelmAction<ActionEdit> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender
          .output(LibraryRowOutput::Edit(index.clone()))
          .expect("LibraryRowOutput receiver dropped");
      })
    };

    // TODO: Implement editing a library name and path
    action_edit.set_enabled(false);

    actions_group.add_action(action_edit);

    let index = self.index.clone();
    relm4::new_stateless_action!(ActionDelete, RowActionGroup, "delete");
    let action_delete: RelmAction<ActionDelete> = {
      let sender = sender.clone();
      RelmAction::new_stateless(move |_| {
        sender
          .output(LibraryRowOutput::Delete(index.clone()))
          .expect("LibraryRowOutput receiver dropped");
      })
    };
    actions_group.add_action(action_delete);

    // Register menu actions for row
    actions_group.register_for_widget(&root);

    widgets
  }
}
