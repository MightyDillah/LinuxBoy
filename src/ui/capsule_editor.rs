use gtk4::prelude::*;
use gtk4::{Box, Button, Dialog, Label, ListBox, Orientation, Paned, ResponseType, ScrolledWindow};
use relm4::RelmWidgetExt;
use std::path::{Path, PathBuf};

use crate::core::capsule::Capsule;

pub struct CapsuleEditor {
    dialog: Dialog,
    capsule: Capsule,
}

impl CapsuleEditor {
    pub fn new(parent: &impl IsA<gtk4::Window>, capsule: &Capsule) -> Self {
        let dialog = Dialog::builder()
            .title(format!("Capsule Editor: {}", capsule.name))
            .modal(true)
            .transient_for(parent)
            .default_width(900)
            .default_height(650)
            .build();

        dialog.add_button("Close", ResponseType::Close);

        let content = Self::build_content(&capsule);
        dialog.content_area().append(&content);

        Self {
            dialog,
            capsule: capsule.clone(),
        }
    }

    fn build_content(capsule: &Capsule) -> Box {
        let vbox = Box::new(Orientation::Vertical, 10);
        vbox.set_margin_all(10);

        // Toolbar
        let toolbar = Box::new(Orientation::Horizontal, 5);

        let extract_btn = Button::with_label("Extract File");
        let add_mod_btn = Button::with_label("Add Mod");
        let open_prefix_btn = Button::with_label("Open Prefix in File Manager");

        toolbar.append(&extract_btn);
        toolbar.append(&add_mod_btn);
        toolbar.append(&open_prefix_btn);

        vbox.append(&toolbar);

        // Paned view: file tree on left, details on right
        let paned = Paned::new(Orientation::Horizontal);

        // Left: File browser
        let left_scroll = ScrolledWindow::new();
        left_scroll.set_vexpand(true);
        left_scroll.set_min_content_width(300);

        let file_tree = Self::build_file_tree(&capsule);
        left_scroll.set_child(Some(&file_tree));

        paned.set_start_child(Some(&left_scroll));

        // Right: File details
        let right_box = Box::new(Orientation::Vertical, 10);
        right_box.set_margin_all(10);

        right_box.append(&Label::new(Some("File Details")));

        let details_label = Label::new(Some("Select a file to view details"));
        details_label.set_halign(gtk4::Align::Start);
        right_box.append(&details_label);

        paned.set_end_child(Some(&right_box));

        vbox.append(&paned);

        // Info section
        let info = Label::new(Some(&format!(
            "AppImage: {:?}\nHome Directory: {:?}",
            capsule.appimage_path, capsule.home_path
        )));
        info.set_halign(gtk4::Align::Start);
        info.set_css_classes(&["dim-label"]);
        vbox.append(&info);

        vbox
    }

    fn build_file_tree(capsule: &Capsule) -> ListBox {
        let list = ListBox::new();

        // Add sections
        list.append(&Label::new(Some("📁 AppImage Contents (Read-Only)")));
        list.append(&Label::new(Some("  └─ game/")));
        list.append(&Label::new(Some("     └─ (mounted read-only)")));

        list.append(&Label::new(Some("\n📁 Wine Prefix (Writable)")));

        if capsule.home_path.exists() {
            if let Ok(entries) = std::fs::read_dir(&capsule.home_path.join("prefix")) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        list.append(&Label::new(Some(&format!("  └─ {}", name))));
                    }
                }
            }
        }

        list.append(&Label::new(Some("\n📁 Cache")));
        list.append(&Label::new(Some("  └─ Shader cache")));

        list
    }

    pub fn run(&self) {
        self.dialog.set_visible(true);
        // Dialog will stay open until user closes it
    }
}

/// Mount an AppImage using FUSE
pub fn mount_appimage(appimage_path: &Path) -> Result<PathBuf, String> {
    let mount_point = std::env::temp_dir().join(format!(
        "linuxboy-mount-{}",
        appimage_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
    ));

    std::fs::create_dir_all(&mount_point).map_err(|e| e.to_string())?;

    let status = std::process::Command::new(appimage_path)
        .arg("--appimage-mount")
        .env("APPIMAGE_EXTRACT_AND_RUN", "1")
        .status()
        .map_err(|e| e.to_string())?;

    if status.success() {
        Ok(mount_point)
    } else {
        Err("Failed to mount AppImage".to_string())
    }
}

/// Unmount an AppImage
pub fn unmount_appimage(mount_point: &Path) -> Result<(), String> {
    let status = std::process::Command::new("fusermount")
        .arg("-u")
        .arg(mount_point)
        .status()
        .map_err(|e| e.to_string())?;

    if status.success() {
        std::fs::remove_dir(mount_point).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("Failed to unmount AppImage".to_string())
    }
}
