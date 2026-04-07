use relm4::RelmApp;

use crate::Result;

pub mod about;
pub mod app;
pub mod prefs;
pub mod tracks_table;

pub fn start() -> Result<()> {
  let app = RelmApp::new("io.github.weiteck.lrc-lyrics");
  Ok(app.run::<app::AppModel>(()))
}
