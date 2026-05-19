use relm4::{
  actions::{AccelsPlus, RelmAction, RelmActionGroup},
  prelude::*,
};

use crate::{settings::APP_NAME_PRETTY, ui::app::AppMsg};

#[derive(Debug, Clone)]
pub(super) struct MainMenuButtonModel;

relm4::new_action_group!(pub(super) MainMenuActionGroup, "main_menu");
relm4::new_stateless_action!(ActionRefreshLibraries, MainMenuActionGroup, "refresh_libraries");
relm4::new_stateless_action!(
  ActionCleanUpSidecarFiles,
  MainMenuActionGroup,
  "clean_up_sidecar_files"
);
relm4::new_stateless_action!(ActionPrefs, MainMenuActionGroup, "prefs");
relm4::new_stateless_action!(ActionAbout, MainMenuActionGroup, "about");
relm4::new_stateless_action!(ActionTestToast, MainMenuActionGroup, "test_toast");
relm4::new_stateless_action!(ActionTestSpinner, MainMenuActionGroup, "test_spinner");

relm4::new_action_group!(pub(super) WindowActionGroup, "window");
relm4::new_stateless_action!(ActionQuit, WindowActionGroup, "quit");
relm4::new_stateless_action!(ActionSearch, WindowActionGroup, "search");
relm4::new_stateless_action!(ActionPinSidebar, WindowActionGroup, "pin_sidebar");

#[relm4::component(pub(super))]
impl SimpleComponent for MainMenuButtonModel {
  type Init = adw::ApplicationWindow;
  type Input = AppMsg;
  type Output = AppMsg;

  view! {
    gtk::MenuButton {
      set_menu_model: Some(&menu),
    },
  }

  fn init(
    app_window: Self::Init,
    root: Self::Root,
    sender: ComponentSender<Self>,
  ) -> ComponentParts<Self> {
    // Main menu actions
    let mut menu_actions_group = RelmActionGroup::<MainMenuActionGroup>::new();

    let sender_handle = sender.clone();
    let action_refresh_libraries: RelmAction<ActionRefreshLibraries> =
      RelmAction::new_stateless(move |_| {
        sender_handle
          .output(AppMsg::RefreshLibraries)
          .expect("MainMenuButtonModel output receiver dropped");
      });
    menu_actions_group.add_action(action_refresh_libraries);

    let sender_handle = sender.clone();
    let action_clean_up_sidecar_files: RelmAction<ActionCleanUpSidecarFiles> =
      RelmAction::new_stateless(move |_| {
        sender_handle
          .output(AppMsg::RequestConfirmCleanUpSidecarFiles)
          .expect("MainMenuButtonModel output receiver dropped");
      });
    menu_actions_group.add_action(action_clean_up_sidecar_files);

    let sender_handle = sender.clone();
    let action_prefs: RelmAction<ActionPrefs> = RelmAction::new_stateless(move |_| {
      sender_handle
        .output(AppMsg::ShowPrefsWindow)
        .expect("MainMenuButtonModel output receiver dropped");
    });
    menu_actions_group.add_action(action_prefs);

    let sender_handle = sender.clone();
    let action_about: RelmAction<ActionAbout> = RelmAction::new_stateless(move |_| {
      sender_handle
        .output(AppMsg::ShowAboutWindow)
        .expect("MainMenuButtonModel output receiver dropped");
    });
    menu_actions_group.add_action(action_about);

    let sender_handle = sender.clone();
    let action_test_toast: RelmAction<ActionTestToast> = RelmAction::new_stateless(move |_| {
      sender_handle
        .output(AppMsg::ShowToast("Testing toast notification".into()))
        .expect("MainMenuButtonModel output receiver dropped");
    });
    menu_actions_group.add_action(action_test_toast);

    let sender_handle = sender.clone();
    let action_test_spinner: RelmAction<ActionTestSpinner> = RelmAction::new_stateless(move |_| {
      sender_handle
        .output(AppMsg::ShowSpinner(("I'm spinning around…".into(), "Get out of my way".into())))
        .expect("MainMenuButtonModel output receiver dropped");
    });
    menu_actions_group.add_action(action_test_spinner);

    // Main menu model
    let menu = gtk::gio::Menu::new();
    menu.append(Some("_Refresh Libraries"), Some("main_menu.refresh_libraries"));
    menu.append(Some("_Clean Up Sidecar Files"), Some("main_menu.clean_up_sidecar_files"));

    let menu_section = gtk::gio::Menu::new();
    menu_section.append(Some("_Preferences"), Some("main_menu.prefs"));
    menu_section.append(Some(&format!("_About {APP_NAME_PRETTY}")), Some("main_menu.about"));
    menu.append_section(None, &menu_section);

    // Add debug menu
    if cfg!(debug_assertions) {
      let debug_menu = gtk::gio::Menu::new();
      debug_menu.append(Some("Test _Toast"), Some("main_menu.test_toast"));
      debug_menu.append(Some("Test _Spinner"), Some("main_menu.test_spinner"));
      let debug_section = gtk::gio::Menu::new();
      debug_section.append_submenu(Some("_Debug"), &debug_menu);
      menu.append_section(None, &debug_section);
    }

    // Keyboard actions
    let mut window_actions_group = RelmActionGroup::<WindowActionGroup>::new();

    let sender_handle = sender.clone();
    let action_quit: RelmAction<ActionQuit> = RelmAction::new_stateless(move |_| {
      sender_handle
        .output(AppMsg::Quit)
        .expect("MainMenuButtonModel output receiver dropped");
    });
    window_actions_group.add_action(action_quit);

    let sender_handle = sender.clone();
    let action_search: RelmAction<ActionSearch> = RelmAction::new_stateless(move |_| {
      sender_handle
        .output(AppMsg::ShowSearch(true))
        .expect("MainMenuButtonModel output receiver dropped");
    });
    window_actions_group.add_action(action_search);

    let sender_handle = sender.clone();
    let pin_sidebar: RelmAction<ActionPinSidebar> = RelmAction::new_stateless(move |_| {
      sender_handle
        .output(AppMsg::TogglePinTrackDetailsSidebar)
        .expect("MainMenuButtonModel output receiver dropped");
    });
    window_actions_group.add_action(pin_sidebar);

    // Keyboard shortcuts
    let app = relm4::main_adw_application();
    app.set_accelerators_for_action::<ActionQuit>(&["<primary>q"]);
    app.set_accelerators_for_action::<ActionSearch>(&["<primary>f"]);
    app.set_accelerators_for_action::<ActionPinSidebar>(&["F9"]);
    app.set_accelerators_for_action::<ActionRefreshLibraries>(&["<primary>r"]);
    app.set_accelerators_for_action::<ActionPrefs>(&["<primary>comma"]);

    // Register actions
    menu_actions_group.register_for_widget(&app_window);
    window_actions_group.register_for_widget(&app_window);

    let model = MainMenuButtonModel;
    let widgets = view_output!();
    ComponentParts { model, widgets }
  }

  fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>) {
    sender
      .output(message)
      .expect("MainMenuButtonModel output receiver dropped");
  }
}
