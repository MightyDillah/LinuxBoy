use gtk4::prelude::*;
use gtk4::{
    Box, Button, ComboBoxText, Dialog, Entry, Label,
    ListBox, Orientation, ResponseType, ScrolledWindow,
};
use relm4::RelmWidgetExt;
use std::path::PathBuf;

pub struct InstallDialog {
    dialog: Dialog,
    game_name: String,
    selected_files: Vec<PathBuf>,
    main_exe: Option<String>,
    launch_args: String,
}

impl InstallDialog {
    pub fn new(parent: &impl IsA<gtk4::Window>) -> Self {
        let dialog = Dialog::builder()
            .title("Install Game")
            .modal(true)
            .transient_for(parent)
            .default_width(700)
            .default_height(600)
            .build();

        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Create Capsule", ResponseType::Accept);

        let content = Self::build_content();
        dialog.content_area().append(&content);

        Self {
            dialog,
            game_name: String::new(),
            selected_files: Vec::new(),
            main_exe: None,
            launch_args: String::new(),
        }
    }

    fn build_content() -> Box {
        let vbox = Box::new(Orientation::Vertical, 15);
        vbox.set_margin_all(20);

        // Title
        let title = Label::new(Some("Create Game Capsule"));
        title.set_css_classes(&["title-2"]);
        vbox.append(&title);

        // Source selection
        vbox.append(&Label::new(Some("Step 1: Select Game Source")));

        let source_box = Box::new(Orientation::Horizontal, 10);

        let portable_btn = Button::with_label("Browse Portable Game Folder");
        source_box.append(&portable_btn);

        let installer_btn = Button::with_label("Browse Installer (.exe, .msi)");
        source_box.append(&installer_btn);

        vbox.append(&source_box);

        let source_label = Label::new(Some("No source selected"));
        source_label.set_halign(gtk4::Align::Start);
        vbox.append(&source_label);

        // Separator
        vbox.append(&gtk4::Separator::new(Orientation::Horizontal));

        // Game configuration
        vbox.append(&Label::new(Some("Step 2: Configure Capsule")));

        let config_grid = gtk4::Grid::new();
        config_grid.set_row_spacing(10);
        config_grid.set_column_spacing(10);

        // Game name
        config_grid.attach(&Label::new(Some("Game Name:")), 0, 0, 1, 1);
        let name_entry = Entry::new();
        name_entry.set_placeholder_text(Some("Enter game name"));
        name_entry.set_hexpand(true);
        config_grid.attach(&name_entry, 1, 0, 1, 1);

        // Main executable selection
        config_grid.attach(&Label::new(Some("Main Executable:")), 0, 1, 1, 1);
        let exe_combo = ComboBoxText::new();
        exe_combo.set_hexpand(true);
        config_grid.attach(&exe_combo, 1, 1, 1, 1);

        // Launch arguments
        config_grid.attach(&Label::new(Some("Launch Arguments:")), 0, 2, 1, 1);
        let args_entry = Entry::new();
        args_entry.set_placeholder_text(Some("-windowed -dx11"));
        args_entry.set_hexpand(true);
        config_grid.attach(&args_entry, 1, 2, 1, 1);

        vbox.append(&config_grid);

        // Separator
        vbox.append(&gtk4::Separator::new(Orientation::Horizontal));

        // Detected executables list
        vbox.append(&Label::new(Some("Detected Executables (for installers)")));

        let scroll = ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_min_content_height(200);

        let exe_list = ListBox::new();
        scroll.set_child(Some(&exe_list));

        vbox.append(&scroll);

        // Info label
        let info = Label::new(Some(
            "Drag and drop a game folder or installer into this window to begin.",
        ));
        info.set_css_classes(&["dim-label"]);
        info.set_margin_top(10);
        vbox.append(&info);

        vbox
    }

    pub fn run(&self) -> Option<InstallConfig> {
        self.dialog.set_visible(true);

        // TODO: Handle file selection and configuration

        self.dialog.close();

        None
    }
}

pub struct InstallConfig {
    pub source_path: PathBuf,
    pub game_name: String,
    pub main_exe: String,
    pub launch_args: String,
    pub is_installer: bool,
}
