use gtk4::prelude::*;
use gtk4::{Dialog, Box, Label, Button, Orientation, Grid, Separator, Frame};
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent};

use crate::core::system_checker::SystemCheck;
use crate::core::runtime_manager::RuntimeManager;

#[derive(Debug)]
pub enum SystemSetupMsg {
    DownloadProton,
    DownloadProgress(String),  // Progress message
    DownloadComplete,
    DownloadError(String),
    Close,
}

pub struct SystemSetupDialog {
    system_check: SystemCheck,
    runtime_mgr: RuntimeManager,
    download_status: String,
    is_downloading: bool,
}

#[relm4::component(pub)]
impl SimpleComponent for SystemSetupDialog {
    type Init = SystemCheck;
    type Input = SystemSetupMsg;
    type Output = ();

    view! {
        #[root]
        Dialog {
            set_title: Some("System Setup"),
            set_modal: true,
            set_default_width: 700,
            set_default_height: 450,

            #[wrap(Some)]
            set_child = &Box {
                set_orientation: Orientation::Vertical,
                set_spacing: 15,
                set_margin_all: 20,

                // Header
                append = &Label {
                    set_markup: "<big><b>LinuxBoy System Setup</b></big>",
                    set_halign: gtk4::Align::Start,
                },

                append = &Label {
                    set_label: "Check and install required components for running Windows games",
                    set_halign: gtk4::Align::Start,
                    set_wrap: true,
                },

                append = &Separator {
                    set_orientation: Orientation::Horizontal,
                },

                // Table header
                append = &Grid {
                    set_column_spacing: 15,
                    set_row_spacing: 12,
                    set_margin_top: 10,

                    // Header row
                    attach[0, 0, 1, 1] = &Label {
                        set_markup: "<b>Component</b>",
                        set_halign: gtk4::Align::Start,
                        set_width_chars: 20,
                    },
                    attach[1, 0, 1, 1] = &Label {
                        set_markup: "<b>Status</b>",
                        set_halign: gtk4::Align::Start,
                        set_width_chars: 25,
                    },
                    attach[2, 0, 1, 1] = &Label {
                        set_markup: "<b>Action</b>",
                        set_halign: gtk4::Align::Start,
                    },

                    // Row 1: Vulkan
                    attach[0, 1, 1, 1] = &Label {
                        set_label: "Vulkan Tools",
                        set_halign: gtk4::Align::Start,
                    },
                    attach[1, 1, 1, 1] = &Label {
                        #[watch]
                        set_markup: if model.system_check.vulkan_installed {
                            "<span foreground='#2ecc71'>✓ Installed</span>"
                        } else {
                            "<span foreground='#e74c3c'>✗ Missing</span>"
                        },
                        set_halign: gtk4::Align::Start,
                    },
                    attach[2, 1, 1, 1] = &Label {
                        #[watch]
                        set_label: if model.system_check.vulkan_installed {
                            ""
                        } else {
                            "Run: sudo apt install vulkan-tools"
                        },
                        set_halign: gtk4::Align::Start,
                        set_wrap: true,
                    },

                    // Row 2: Mesa
                    attach[0, 2, 1, 1] = &Label {
                        set_label: "Mesa Drivers",
                        set_halign: gtk4::Align::Start,
                    },
                    attach[1, 2, 1, 1] = &Label {
                        #[watch]
                        set_markup: if model.system_check.mesa_installed {
                            "<span foreground='#2ecc71'>✓ Installed</span>"
                        } else {
                            "<span foreground='#e74c3c'>✗ Missing</span>"
                        },
                        set_halign: gtk4::Align::Start,
                    },
                    attach[2, 2, 1, 1] = &Label {
                        #[watch]
                        set_label: if model.system_check.mesa_installed {
                            ""
                        } else {
                            "Run: sudo apt install mesa-vulkan-drivers"
                        },
                        set_halign: gtk4::Align::Start,
                        set_wrap: true,
                    },

                    // Row 3: Proton-GE
                    attach[0, 3, 1, 1] = &Label {
                        set_label: "Proton-GE",
                        set_halign: gtk4::Align::Start,
                    },
                    attach[1, 3, 1, 1] = &Label {
                        #[watch]
                        set_markup: if model.system_check.proton_installed {
                            "<span foreground='#2ecc71'>✓ Installed</span>"
                        } else {
                            "<span foreground='#f39c12'>✗ Not Downloaded</span>"
                        },
                        set_halign: gtk4::Align::Start,
                    },
                    attach[2, 3, 1, 1] = &Button {
                        #[watch]
                        set_label: if model.is_downloading {
                            "Downloading..."
                        } else {
                            "Download Latest"
                        },
                        #[watch]
                        set_visible: !model.system_check.proton_installed,
                        #[watch]
                        set_sensitive: !model.is_downloading,
                        connect_clicked => SystemSetupMsg::DownloadProton,
                    },

                    // Row 4: Wine
                    attach[0, 4, 1, 1] = &Label {
                        set_label: "Wine",
                        set_halign: gtk4::Align::Start,
                    },
                    attach[1, 4, 1, 1] = &Label {
                        #[watch]
                        set_markup: if model.system_check.wine_installed {
                            "<span foreground='#2ecc71'>✓ Installed</span>"
                        } else {
                            "<span foreground='#f39c12'>⚠ Optional</span>"
                        },
                        set_halign: gtk4::Align::Start,
                    },
                    attach[2, 4, 1, 1] = &Label {
                        #[watch]
                        set_label: if model.system_check.wine_installed {
                            ""
                        } else {
                            "Optional: Run sudo apt install wine"
                        },
                        set_halign: gtk4::Align::Start,
                        set_wrap: true,
                    },
                },

                // Missing APT packages section
                append = &Box {
                    set_orientation: Orientation::Vertical,
                    set_spacing: 10,
                    set_margin_top: 15,
                    #[watch]
                    set_visible: !model.system_check.missing_apt_packages.is_empty(),

                    append = &Separator {
                        set_orientation: Orientation::Horizontal,
                    },

                    append = &Label {
                        set_markup: "<b>Missing System Packages</b>",
                        set_halign: gtk4::Align::Start,
                    },

                    append = &Label {
                        set_label: "Install the following packages to enable Vulkan support:",
                        set_halign: gtk4::Align::Start,
                        set_wrap: true,
                    },

                    append = &Frame {
                        set_margin_top: 5,
                        #[wrap(Some)]
                        set_child = &Box {
                            set_orientation: Orientation::Vertical,
                            set_margin_all: 10,

                            append = &Label {
                                #[watch]
                                set_markup: &format!("<tt>sudo apt install {}</tt>", 
                                    model.system_check.missing_apt_packages.join(" ")
                                ),
                                set_halign: gtk4::Align::Start,
                                set_selectable: true,
                                set_wrap: true,
                            },
                        },
                    },
                },

                // Download status area
                append = &Box {
                    set_orientation: Orientation::Vertical,
                    set_spacing: 5,
                    set_margin_top: 10,
                    #[watch]
                    set_visible: !model.download_status.is_empty(),

                    append = &Separator {
                        set_orientation: Orientation::Horizontal,
                    },

                    append = &Label {
                        set_markup: "<b>Download Status</b>",
                        set_halign: gtk4::Align::Start,
                    },

                    append = &Label {
                        #[watch]
                        set_label: &model.download_status,
                        set_halign: gtk4::Align::Start,
                        set_wrap: true,
                    },
                },

                // Spacer
                append = &Box {
                    set_vexpand: true,
                },

                append = &Separator {
                    set_orientation: Orientation::Horizontal,
                },

                // Bottom buttons
                append = &Box {
                    set_orientation: Orientation::Horizontal,
                    set_spacing: 10,
                    set_halign: gtk4::Align::End,

                    append = &Button {
                        set_label: "Close",
                        connect_clicked => SystemSetupMsg::Close,
                    },
                },
            },
        }
    }

    fn init(
        system_check: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let runtime_mgr = RuntimeManager::new();

        let model = SystemSetupDialog {
            system_check,
            runtime_mgr,
            download_status: String::new(),
            is_downloading: false,
        };

        let widgets = view_output!();
        
        // Show the dialog
        root.present();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            SystemSetupMsg::DownloadProton => {
                println!("Starting Proton-GE download in background...");
                self.is_downloading = true;
                self.download_status = "Fetching latest release information...".to_string();
                
                // Clone what we need for the background thread
                let runtime_mgr = self.runtime_mgr.clone();
                let sender_clone = sender.clone();
                
                // Spawn background thread for download
                std::thread::spawn(move || {
                    // Update progress: Fetching release info
                    let _ = sender_clone.input(SystemSetupMsg::DownloadProgress(
                        "Fetching latest Proton-GE release...".to_string()
                    ));
                    
                    match runtime_mgr.get_latest_release() {
                        Ok(release) => {
                            println!("Found release: {}", release.tag_name);
                            
                            // Update progress: Starting download
                            let _ = sender_clone.input(SystemSetupMsg::DownloadProgress(
                                format!("Downloading {} (this may take several minutes)...", release.tag_name)
                            ));
                            
                            match runtime_mgr.install_proton_ge(&release) {
                                Ok(path) => {
                                    println!("✓ Proton-GE installed successfully to: {:?}", path);
                                    let _ = sender_clone.input(SystemSetupMsg::DownloadComplete);
                                }
                                Err(e) => {
                                    eprintln!("✗ Download failed: {}", e);
                                    let _ = sender_clone.input(SystemSetupMsg::DownloadError(
                                        format!("Installation failed: {}", e)
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("✗ Failed to fetch releases: {}", e);
                            let _ = sender_clone.input(SystemSetupMsg::DownloadError(
                                format!("Failed to fetch releases: {}", e)
                            ));
                        }
                    }
                });
            }
            
            SystemSetupMsg::DownloadProgress(status) => {
                self.download_status = status;
            }
            
            SystemSetupMsg::DownloadComplete => {
                self.is_downloading = false;
                self.download_status = "✓ Proton-GE installed successfully!".to_string();
                // Refresh system check
                self.system_check = SystemCheck::check();
            }
            
            SystemSetupMsg::DownloadError(error) => {
                self.is_downloading = false;
                self.download_status = format!("✗ Error: {}", error);
            }
            
            SystemSetupMsg::Close => {
                // Dialog closes when button is clicked
                println!("Closing system setup dialog");
            }
        }
    }
}
