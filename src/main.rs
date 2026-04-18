#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use lyrinc::{init_app, ui};
use mimalloc::MiMalloc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  init_app().await?;
  ui::app::start()
}
