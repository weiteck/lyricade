#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use lyricade::ui;
use mimalloc::MiMalloc;

fn main() -> anyhow::Result<()> {
  ui::app::start()
}
