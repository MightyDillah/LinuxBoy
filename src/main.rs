mod core;
mod ui;
mod utils;

use relm4::{RelmApp, set_global_css};
use ui::main_window::MainWindow;

fn main() {
    let app = RelmApp::new("com.linuxboy.app");
    set_global_css(include_str!("ui/style.css"));
    app.run::<MainWindow>(());
}
