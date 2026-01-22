mod core;
mod ui;
mod utils;

use gtk4::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender, RelmApp, SimpleComponent};

use ui::main_window::MainWindow;

fn main() {
    let app = RelmApp::new("com.linuxboy.app");
    app.run::<MainWindow>(());
}
