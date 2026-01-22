use gtk4::prelude::*;
use gtk4::{Dialog, Box, Label, Button, Orientation, Grid, Separator, Frame, ProgressBar};
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent};

use crate::core::system_checker::SystemCheck;
use crate::core::runtime_manager::RuntimeManager;

#[derive(Debug)]
pub enum SystemSetupMsg {
    DownloadProton,
    DownloadProgress { status: String, progress: f64 },  // status text and 0.0-1.0 progress
    DownloadVersion(String),
    DownloadComplete,
    DownloadError(String),
    Refresh(SystemCheck),
    Close,
}

#[derive(Debug)]
pub enum SystemSetupOutput {
    CloseRequested,
}

pub struct SystemSetupDialog {
    system_check: SystemCheck,
    runtime_mgr: RuntimeManager,
    download_status: String,
    download_progress: f64,  // 0.0 to 1.0
    download_version: Option<String>,
    is_downloading: bool,
}

#[relm4::component(pub)]
impl SimpleComponent for SystemSetupDialog {
    type Init = SystemCheck;
    type Input = SystemSetupMsg;
    type Output = SystemSetupOutput;

    view! {
        #[root]
        Dialog {
            set_title: Some("System Setup"),
            set_modal: true,
            set_default_width: 700,
            set_default_height: 500,
            set_hide_on_close: true,

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
                    set_spacing: 10,
                    set_margin_top: 10,
                    #[watch]
                    set_visible: model.is_downloading || !model.download_status.is_empty(),

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

                    append = &Label {
                        #[watch]
                        set_visible: model.download_version.is_some(),
                        #[watch]
                        set_label: &model
                            .download_version
                            .as_deref()
                            .map(|version| format!("Version: {}", version))
                            .unwrap_or_default(),
                        set_halign: gtk4::Align::Start,
                    },

                    append = &ProgressBar {
                        #[watch]
                        set_visible: model.is_downloading,
                        #[watch]
                        set_fraction: model.download_progress,
                        set_show_text: true,
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
            download_progress: 0.0,
            download_version: None,
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
                self.download_progress = 0.0;
                self.download_version = None;
                
                let runtime_mgr = self.runtime_mgr.clone();
                let sender_clone = sender.clone();
                
                enum DownloadUpdate {
                    Progress { status: String, progress: f64 },
                    Version(String),
                    Complete,
                    Error(String),
                }

                // Create a channel for progress updates
                let (tx, rx) = std::sync::mpsc::channel::<DownloadUpdate>();
                
                // Spawn blocking thread for download
                std::thread::spawn(move || {
                    // Fetch release info
                    match runtime_mgr.get_latest_release() {
                        Ok(release) => {
                            println!("Found release: {}", release.tag_name);
                            let _ = tx.send(DownloadUpdate::Version(release.tag_name.clone()));
                            let _ = tx.send(DownloadUpdate::Progress {
                                status: format!("Preparing {} download...", release.tag_name),
                                progress: 0.0,
                            });
                            
                            // Install with progress callbacks that send to channel
                            match runtime_mgr.install_proton_ge(&release, |status, progress| {
                                let _ = tx.send(DownloadUpdate::Progress { status, progress });
                            }) {
                                Ok(path) => {
                                    println!("✓ Proton-GE installed successfully to: {:?}", path);
                                    let _ = tx.send(DownloadUpdate::Complete);
                                }
                                Err(e) => {
                                    eprintln!("✗ Installation failed: {}", e);
                                    let _ = tx.send(DownloadUpdate::Error(e.to_string()));
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("✗ Failed to fetch releases: {}", e);
                            let _ = tx.send(DownloadUpdate::Error(format!("Failed to fetch releases: {}", e)));
                        }
                    }
                });
                
                // Poll the channel from GTK main thread
                glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                    // Drain all available messages
                    let mut last_msg = None;
                    while let Ok(msg) = rx.try_recv() {
                        last_msg = Some(msg);
                    }
                    
                    if let Some(update) = last_msg {
                        match update {
                            DownloadUpdate::Progress { status, progress } => {
                                let _ = sender_clone.input(SystemSetupMsg::DownloadProgress {
                                    status,
                                    progress,
                                });
                            }
                            DownloadUpdate::Version(version) => {
                                let _ = sender_clone.input(SystemSetupMsg::DownloadVersion(version));
                            }
                            DownloadUpdate::Complete => {
                                let _ = sender_clone.input(SystemSetupMsg::DownloadComplete);
                                return glib::ControlFlow::Break;
                            }
                            DownloadUpdate::Error(error) => {
                                let _ = sender_clone.input(SystemSetupMsg::DownloadError(error));
                                return glib::ControlFlow::Break;
                            }
                        }
                    }
                    
                    glib::ControlFlow::Continue
                });
            }
            
            SystemSetupMsg::DownloadVersion(version) => {
                self.download_version = Some(version);
            }

            SystemSetupMsg::DownloadProgress { status, progress } => {
                self.download_status = status;
                self.download_progress = progress;
            }
            
            SystemSetupMsg::DownloadComplete => {
                self.is_downloading = false;
                let version = self
                    .download_version
                    .as_deref()
                    .unwrap_or("latest");
                self.download_status = format!("✓ Proton-GE {} installed successfully!", version);
                self.download_progress = 1.0;
                // Refresh system check
                self.system_check = SystemCheck::check();
            }
            
            SystemSetupMsg::DownloadError(error) => {
                self.is_downloading = false;
                self.download_status = format!("✗ Error: {}", error);
                self.download_progress = 0.0;
            }

            SystemSetupMsg::Refresh(system_check) => {
                self.system_check = system_check;
            }
            
            SystemSetupMsg::Close => {
                // Dialog closes when button is clicked
                println!("Closing system setup dialog");
                let _ = sender.output(SystemSetupOutput::CloseRequested);
            }
        }
    }
}
