use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Box, Button, Label, Orientation, ScrolledWindow};
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent};

use crate::core::capsule::Capsule;
use crate::core::system_checker::{SystemCheck, SystemStatus};
use crate::core::runtime_manager::RuntimeManager;
use std::path::PathBuf;

#[derive(Debug)]
pub enum MainWindowMsg {
    LoadCapsules,
    OpenInstaller,
    OpenSystemSetup,
}

pub struct MainWindow {
    capsules: Vec<Capsule>,
    games_dir: PathBuf,
    system_check: SystemCheck,
}

#[relm4::component(pub)]
impl SimpleComponent for MainWindow {
    type Init = ();
    type Input = MainWindowMsg;
    type Output = ();

    view! {
        #[root]
        ApplicationWindow {
            set_title: Some("LinuxBoy"),
            set_default_width: 1000,
            set_default_height: 700,

            #[wrap(Some)]
            set_child = &Box {
                set_orientation: Orientation::Vertical,
                set_spacing: 0,

                // Header bar
                append = &Box {
                    set_orientation: Orientation::Horizontal,
                    set_spacing: 10,
                    set_margin_all: 10,

                    append = &Label {
                        set_label: "LinuxBoy Gaming Manager",
                        set_css_classes: &["title"],
                    },

                    append = &Box {
                        set_hexpand: true,
                    },

                    append = &Button {
                        set_label: "+ Add Game",
                        connect_clicked => MainWindowMsg::OpenInstaller,
                    },
                },

                // Main content
                append = &Box {
                    set_orientation: Orientation::Horizontal,
                    set_spacing: 0,
                    set_hexpand: true,
                    set_vexpand: true,

                    // Sidebar
                    append = &Box {
                        set_orientation: Orientation::Vertical,
                        set_width_request: 200,
                        set_css_classes: &["sidebar"],

                        append = &Button {
                            set_label: "Library",
                            set_margin_all: 5,
                        },

                        append = &Button {
                            set_label: "System Check",
                            set_margin_all: 5,
                        },

                        append = &Button {
                            set_label: "Settings",
                            set_margin_all: 5,
                        },
                    },

                    // Main content area
                    append = &ScrolledWindow {
                        set_hexpand: true,
                        set_vexpand: true,

                        #[wrap(Some)]
                        set_child = &gtk4::FlowBox {
                            set_valign: gtk4::Align::Start,
                            set_max_children_per_line: 4,
                            set_selection_mode: gtk4::SelectionMode::None,
                            set_margin_all: 20,
                            set_row_spacing: 20,
                            set_column_spacing: 20,
                        },
                    },
                },

                // Status bar
                append = &Box {
                    set_orientation: Orientation::Horizontal,
                    set_spacing: 10,
                    set_margin_all: 5,

                    append = &Label {
                        #[watch]
                        set_label: &format!("{} games installed", model.capsules.len()),
                    },

                    append = &Box {
                        set_hexpand: true,
                    },

                    // System status indicator
                    append = &Button {
                        #[watch]
                        set_label: &match model.system_check.status {
                            SystemStatus::AllInstalled => "🟢 System Ready",
                            SystemStatus::PartiallyInstalled => "🟠 Setup Incomplete",
                            SystemStatus::NothingInstalled => "🔴 Setup Required",
                        },
                        set_tooltip_text: Some(&model.system_check.status_message()),
                        connect_clicked => MainWindowMsg::OpenSystemSetup,
                    },
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let games_dir = dirs::home_dir()
            .unwrap_or_default()
            .join("Games");

        // Check system on startup
        let system_check = SystemCheck::check();
        println!("System check: {:?}", system_check.status);

        let model = MainWindow {
            capsules: Vec::new(),
            games_dir,
            system_check,
        };

        let widgets = view_output!();

        // Load capsules on startup
        sender.input(MainWindowMsg::LoadCapsules);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            MainWindowMsg::LoadCapsules => {
                match Capsule::scan_directory(&self.games_dir) {
                    Ok(capsules) => {
                        self.capsules = capsules;
                        println!("Loaded {} capsules", self.capsules.len());
                    }
                    Err(e) => {
                        eprintln!("Failed to load capsules: {}", e);
                    }
                }
            }
            MainWindowMsg::OpenInstaller => {
                println!("Open installer dialog");
                // TODO: Implement installer dialog
            }
            MainWindowMsg::OpenSystemSetup => {
                // Re-check system status
                self.system_check = SystemCheck::check();
                
                // Print detailed status
                println!("\n=== System Status ===");
                for line in self.system_check.detailed_status() {
                    println!("{}", line);
                }
                
                // Show runtime manager info
                let runtime_mgr = RuntimeManager::new();
                println!("\n=== Runtime Manager ===");
                match runtime_mgr.list_installed() {
                    Ok(installed) => {
                        if installed.is_empty() {
                            println!("No Proton-GE versions installed");
                        } else {
                            println!("Installed versions:");
                            for version in installed {
                                println!("  - {}", version);
                            }
                        }
                    }
                    Err(e) => println!("Error checking installed versions: {}", e),
                }
                
                // List available releases from GitHub
                println!("\nFetching available releases from GitHub...");
                match runtime_mgr.fetch_available_releases() {
                    Ok(releases) => {
                        println!("Available Proton-GE releases (showing first 5):");
                        for release in releases.iter().take(5) {
                            println!("  - {} ({})", release.tag_name, release.published_at);
                            if let Some(asset) = RuntimeManager::find_targz_asset(release) {
                                println!("    File: {} ({} MB)", 
                                    asset.name, 
                                    asset.size / 1_048_576);
                            }
                        }
                    }
                    Err(e) => println!("Error fetching releases: {}", e),
                }
                
                println!("====================\n");
                
                // TODO: Open system setup dialog GUI
                println!("System setup dialog (GUI coming soon)...");
            }
        }
    }
}
