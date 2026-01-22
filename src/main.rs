mod core;
mod ui;
mod utils;

use relm4::RelmApp;
use ui::main_window::MainWindow;

fn main() {
    let app = RelmApp::new("com.linuxboy.app");
    app.run::<MainWindow>(());
}
