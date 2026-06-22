use adw::prelude::*;
use camino::Utf8PathBuf;
use relm4::prelude::*;
use tracing::{debug, trace};

use crate::library::Library;

pub(super) struct LibraryRow {
  pub(crate) index: DynamicIndex,
  pub(crate) library: Library,

  pub(crate) name_initial: Option<String>,
  pub(crate) path_initial: String,

  pub(crate) is_modified: bool,
  pub(crate) name_too_long: bool,

  pub(crate) sender: FactorySender<LibraryRow>,
}

#[derive(Debug)]
pub(super) enum LibraryRowMsg {
  UpdateName(String),
  ValidateNameEntry,
  FileDialogRequest,
  UpdatePath(Utf8PathBuf),
  Delete,
  Save,
  Cancel,
}

#[derive(Debug)]
pub(super) enum LibraryRowOutput {
  Delete(DynamicIndex),
  FileDialogRequest(DynamicIndex),
  ShowToast(String, bool),
}

#[relm4::factory(pub)]
impl FactoryComponent for LibraryRow {
  type Init = Library;
  type Input = LibraryRowMsg;
  type Output = LibraryRowOutput;
  type CommandOutput = ();
  type ParentWidget = gtk::ListBox;

  view! {
    #[name = "expander_row"]
    adw::ExpanderRow {
      set_valign: gtk::Align::Center,
      set_focusable: false,
      set_selectable: false,
      set_use_markup: false,
      set_title: &self.library.name(),
      set_subtitle: &self.library.path,

      connect_expanded_notify[sender] => move |er| {
        if !er.is_expanded() {
          sender.input(LibraryRowMsg::Cancel);
        }
      },

      #[name = "name_entry_row"]
      add_row = &adw::EntryRow {
        set_editable: true,
        set_use_markup: false,
        set_title: "Name",
        set_text: &self.library.name(),

        connect_changed[sender] => move |er| {
          sender.input(LibraryRowMsg::UpdateName(er.text().to_string()));
        },

        add_controller = gtk::EventControllerFocus {
          connect_leave[sender] => move |_| {
            sender.input(LibraryRowMsg::ValidateNameEntry);
          },
        },
      },

      #[name = "path_entry_row"]
      add_row = &adw::ActionRow {
        set_valign: gtk::Align::Center,
        add_css_class: "property", // reverse title/subtitle styling
        set_use_markup: false,
        set_title: "Library Path",
        set_subtitle: &self.library.path,

        add_suffix = &gtk::Button {
          set_valign: gtk::Align::Center,
          set_vexpand: false,
          set_icon_name: "folder-open-symbolic",
          add_css_class: "flat",
          set_tooltip: "Browse",
          connect_clicked => LibraryRowMsg::FileDialogRequest,
        },
      },

      // Buttons row
      // Manually create the `ListBoxRow` so we can disable the hover effect
      add_row = &gtk::ListBoxRow {
        set_selectable: false,
        set_activatable: false,

        gtk::Box {
          set_margin_all: 12,
          set_spacing: 6,
          set_homogeneous: true,

          #[name = "save_button"]
          gtk::Button {
            set_hexpand: true,
            set_label: "Save",
            set_sensitive: false,
            connect_clicked => LibraryRowMsg::Save,
          },

          gtk::Button {
            set_hexpand: true,
            set_label: "Cancel",
            connect_clicked => LibraryRowMsg::Cancel,
          },

          gtk::Button {
            set_hexpand: true,
            set_label: "Delete Library",
            add_css_class: "destructive-action",
            connect_clicked => LibraryRowMsg::Delete,
          },
        },
      },
    },
  }

  fn init_model(library: Self::Init, index: &Self::Index, sender: FactorySender<Self>) -> Self {
    Self {
      index: index.clone(),
      name_initial: library.name.clone(),
      path_initial: library.path.clone(),
      library,
      is_modified: false,
      name_too_long: false,
      sender,
    }
  }

  fn init_widgets(
    &mut self,
    index: &Self::Index,
    root: Self::Root,
    _parent: &<Self::ParentWidget as relm4::factory::FactoryView>::ReturnedWidget,
    sender: FactorySender<Self>,
  ) -> Self::Widgets {
    trace!("Building LibraryRow for {} at index {}", &self.library, &index.current_index());

    let widgets = view_output!();
    widgets
  }

  fn update_with_view(
    &mut self,
    widgets: &mut Self::Widgets,
    message: Self::Input,
    sender: FactorySender<Self>,
  ) {
    match message {
      LibraryRowMsg::UpdateName(name) => {
        debug!("Called UpdateName to \"{}\" on LibraryRow for {}", &name, self.library);

        // Limit length of name
        if name.len() > 60 {
          if !self.name_too_long {
            sender
              .output(LibraryRowOutput::ShowToast(
                "Name too long (max. 60 characters)".into(),
                true,
              ))
              .expect("LibraryRowOutput receiver dropped");
          }
          self.name_too_long = true;
          widgets.name_entry_row.add_css_class("error");
          widgets.save_button.set_sensitive(false);
          widgets.save_button.remove_css_class("suggested-action");
        } else {
          if name.is_empty() || name == self.library.default_name() {
            self.library.name = None;
          } else {
            self.library.name = Some(name);
          }

          self.update_modified();
          self.name_too_long = false;
          widgets.name_entry_row.remove_css_class("error");

          if self.is_modified {
            widgets.save_button.set_sensitive(true);
            widgets.save_button.add_css_class("suggested-action");
          } else {
            widgets.save_button.set_sensitive(false);
            widgets.save_button.remove_css_class("suggested-action");
          }
        }
      }

      LibraryRowMsg::ValidateNameEntry => {
        debug!("Called NameEntryValidate on LibraryRow for {}", self.library);

        // Use default library name if set to empty
        if widgets.name_entry_row.text().is_empty() {
          widgets
            .name_entry_row
            .set_text(&self.library.default_name());
        }
      }

      LibraryRowMsg::FileDialogRequest => {
        debug!("Called FileDialogRequest on LibraryRow for {}", self.library);

        sender
          .output(LibraryRowOutput::FileDialogRequest(self.index.clone()))
          .expect("LibraryRowOutput receiver dropped");
      }

      LibraryRowMsg::UpdatePath(path) => {
        debug!("Called UpdatePath to \"{}\" on LibraryRow for {}", &path, self.library);

        if let Err(error) = self.library.set_path(&path) {
          sender
            .output(LibraryRowOutput::ShowToast(error.to_string(), true))
            .expect("LibraryRowOutput receiver dropped");
        } else {
          widgets.path_entry_row.set_subtitle(path.as_str());

          self.update_modified();

          if self.is_modified {
            widgets.save_button.set_sensitive(true);
            widgets.save_button.add_css_class("suggested-action");
          } else {
            widgets.save_button.set_sensitive(false);
            widgets.save_button.remove_css_class("suggested-action");
          }
        }
      }

      LibraryRowMsg::Delete => {
        debug!("Called Delete on LibraryRow for {}", self.library);

        self
          .library
          .remove()
          .expect("failed to delete library from database");

        // Tell parent to remove the row
        sender
          .output(LibraryRowOutput::Delete(self.index.clone()))
          .expect("LibraryRowOutput receiver dropped");

        sender
          .output(LibraryRowOutput::ShowToast(
            format!("Library “{}” deleted", self.library.name()),
            false,
          ))
          .expect("LibraryRowOutput receiver dropped");
      }

      LibraryRowMsg::Save => {
        debug!("Called Save on LibraryRow for {}", self.library);

        self
          .library
          .write_to_db()
          .call()
          .expect("failed to write library to database");

        // Update row values and collapse
        widgets.expander_row.set_title(&self.library.name());
        widgets.expander_row.set_subtitle(&self.library.path);
        widgets.expander_row.set_expanded(false);

        self.is_modified = false;
      }

      LibraryRowMsg::Cancel => {
        debug!("Called Cancel on LibraryRow for {}", self.library);

        widgets.expander_row.set_expanded(false);

        if self.is_modified {
          self.library.name = self.name_initial.clone();
          self.library.path = self.path_initial.clone();

          widgets.name_entry_row.set_text(&self.library.name());

          self.is_modified = false;
        }
      }
    }
  }
}

impl LibraryRow {
  fn update_modified(&mut self) {
    self.is_modified =
      (&self.name_initial, &self.path_initial) != (&self.library.name, &self.library.path);
  }
}
