use gtk4::prelude::*;
use gtk4::{
    Box, Button, CheckButton, Dialog, Entry, Grid, Label, Orientation, ResponseType, Switch,
};
use relm4::RelmWidgetExt;

use crate::core::capsule::{Capsule, CapsuleMetadata};

pub struct CapsulePropertiesDialog {
    dialog: Dialog,
    metadata: CapsuleMetadata,
}

impl CapsulePropertiesDialog {
    pub fn new(parent: &impl IsA<gtk4::Window>, capsule: &Capsule) -> Self {
        let dialog = Dialog::builder()
            .title(format!("Properties: {}", capsule.name))
            .modal(true)
            .transient_for(parent)
            .default_width(600)
            .default_height(500)
            .build();

        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Save", ResponseType::Accept);

        let content = Self::build_content(&capsule.metadata);
        dialog.content_area().append(&content);

        Self {
            dialog,
            metadata: capsule.metadata.clone(),
        }
    }

    fn build_content(metadata: &CapsuleMetadata) -> Box {
        let vbox = Box::new(Orientation::Vertical, 10);
        vbox.set_margin_all(20);

        // Game name
        vbox.append(&Label::new(Some("Game Information")));
        vbox.append(&Self::create_separator());

        let name_box = Box::new(Orientation::Horizontal, 10);
        name_box.append(&Label::new(Some("Name:")));
        let name_entry = Entry::new();
        name_entry.set_text(&metadata.name);
        name_entry.set_hexpand(true);
        name_box.append(&name_entry);
        vbox.append(&name_box);

        // Launch configuration
        vbox.append(&Label::new(Some("\nLaunch Configuration")));
        vbox.append(&Self::create_separator());

        let exe_box = Box::new(Orientation::Horizontal, 10);
        exe_box.append(&Label::new(Some("Executable:")));
        let exe_entry = Entry::new();
        exe_entry.set_text(&metadata.executables.main.path);
        exe_entry.set_hexpand(true);
        exe_box.append(&exe_entry);
        vbox.append(&exe_box);

        let args_box = Box::new(Orientation::Horizontal, 10);
        args_box.append(&Label::new(Some("Arguments:")));
        let args_entry = Entry::new();
        args_entry.set_text(&metadata.executables.main.args);
        args_entry.set_hexpand(true);
        args_box.append(&args_entry);
        vbox.append(&args_box);

        // Wine configuration
        vbox.append(&Label::new(Some("\nWine Configuration")));
        vbox.append(&Self::create_separator());

        let wine_box = Box::new(Orientation::Horizontal, 10);
        wine_box.append(&Label::new(Some("Wine Version:")));
        let wine_entry = Entry::new();
        wine_entry.set_text(metadata.wine_version.as_deref().unwrap_or("wine64"));
        wine_entry.set_hexpand(true);
        wine_box.append(&wine_entry);
        vbox.append(&wine_box);

        // DXVK/VKD3D
        let renderer_grid = Grid::new();
        renderer_grid.set_row_spacing(10);
        renderer_grid.set_column_spacing(10);

        let dxvk_label = Label::new(Some("DXVK (DirectX 9-11):"));
        dxvk_label.set_halign(gtk4::Align::Start);
        renderer_grid.attach(&dxvk_label, 0, 0, 1, 1);

        let dxvk_switch = Switch::new();
        dxvk_switch.set_active(metadata.dxvk_enabled);
        renderer_grid.attach(&dxvk_switch, 1, 0, 1, 1);

        let vkd3d_label = Label::new(Some("VKD3D-Proton (DirectX 12):"));
        vkd3d_label.set_halign(gtk4::Align::Start);
        renderer_grid.attach(&vkd3d_label, 0, 1, 1, 1);

        let vkd3d_switch = Switch::new();
        vkd3d_switch.set_active(metadata.vkd3d_enabled);
        renderer_grid.attach(&vkd3d_switch, 1, 1, 1, 1);

        vbox.append(&renderer_grid);

        // Redistributables
        vbox.append(&Label::new(Some("\nRedistributables")));
        vbox.append(&Self::create_separator());

        let redist_box = Box::new(Orientation::Vertical, 5);

        for redist in &["Visual C++ Runtime", ".NET Framework 4.8", "DirectX End-User Runtime"] {
            let check = CheckButton::with_label(redist);
            redist_box.append(&check);
        }

        let install_button = Button::with_label("Install Selected");
        redist_box.append(&install_button);

        vbox.append(&redist_box);

        vbox
    }

    fn create_separator() -> gtk4::Separator {
        gtk4::Separator::new(Orientation::Horizontal)
    }

    pub fn run(&self) -> Option<CapsuleMetadata> {
        self.dialog.set_visible(true);

        // TODO: Extract values from UI and update metadata

        self.dialog.close();

        // For now, return the original metadata
        // In a real implementation, we'd extract values from the UI widgets
        Some(self.metadata.clone())
    }
}
