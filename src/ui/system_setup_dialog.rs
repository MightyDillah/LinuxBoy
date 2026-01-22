use gtk4::prelude::*;
use gtk4::{Dialog, Box, Label, Button, Image, Orientation, ProgressBar};
use gtk4::gdk;
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent};
use std::process::Command;

use crate::core::system_checker::SystemCheck;
use crate::core::runtime_manager::RuntimeManager;

#[derive(Debug)]
pub enum SystemSetupMsg {
    DownloadProton { reinstall: bool },
    DownloadProgress { status: String, progress: f64 },  // status text and 0.0-1.0 progress
    DownloadVersion(String),
    DownloadComplete,
    DownloadError(String),
    CopySetupScript { reinstall: bool },
    RefreshStatus,
    Refresh(SystemCheck),
    Close,
}

#[derive(Debug)]
pub enum SystemSetupOutput {
    CloseRequested,
    SystemCheckUpdated(SystemCheck),
}

pub struct SystemSetupDialog {
    system_check: SystemCheck,
    runtime_mgr: RuntimeManager,
    download_status: String,
    download_progress: f64,  // 0.0 to 1.0
    download_version: Option<String>,
    is_downloading: bool,
    proton_installed_version: Option<String>,
    umu_installed_version: Option<String>,
    umu_status_markup: String,
    proton_status_markup: String,
}

impl SystemSetupDialog {
    fn setup_script_command(reinstall: bool) -> String {
        if reinstall {
            "bash ./scripts/linuxboy-setup.sh --reinstall".to_string()
        } else {
            "bash ./scripts/linuxboy-setup.sh".to_string()
        }
    }

    fn copy_to_clipboard(text: &str) {
        if let Some(display) = gdk::Display::default() {
            display.clipboard().set_text(text);
        }
    }

    fn command_output(cmd: &str, args: &[&str]) -> Option<String> {
        let output = Command::new(cmd).args(args).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    fn detect_umu_version() -> Option<String> {
        if let Some(version) = Self::command_output(
            "dpkg-query",
            &["-W", "-f=${Version}", "python3-umu-launcher"],
        ) {
            return Some(version);
        }
        Self::command_output("umu-run", &["--version"])
    }

    fn update_status_markup(&mut self) {
        self.umu_status_markup = if self.system_check.umu_installed {
            if let Some(version) = &self.umu_installed_version {
                format!("<span foreground='#2ecc71'>✓ Installed ({})</span>", version)
            } else {
                "<span foreground='#2ecc71'>✓ Installed</span>".to_string()
            }
        } else {
            "<span foreground='#e74c3c'>✗ Missing</span>".to_string()
        };

        self.proton_status_markup = if self.system_check.proton_installed {
            if let Some(version) = &self.proton_installed_version {
                format!("<span foreground='#2ecc71'>✓ Installed ({})</span>", version)
            } else {
                "<span foreground='#2ecc71'>✓ Installed</span>".to_string()
            }
        } else {
            "<span foreground='#f39c12'>✗ Not Downloaded</span>".to_string()
        };
    }
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
                set_spacing: 16,
                set_margin_all: 20,
                set_css_classes: &["dialog-root"],

                // Header
                append = &Box {
                    set_orientation: Orientation::Horizontal,
                    set_spacing: 12,

                    append = &Image {
                        set_icon_name: Some("preferences-system-symbolic"),
                        set_pixel_size: 28,
                    },

                    append = &Box {
                        set_orientation: Orientation::Vertical,
                        set_spacing: 4,

                        append = &Label {
                            set_label: "System Setup",
                            set_css_classes: &["app-title"],
                            set_halign: gtk4::Align::Start,
                        },

                        append = &Label {
                            set_label: "Check and install required components for running Windows games.",
                            set_css_classes: &["muted"],
                            set_halign: gtk4::Align::Start,
                            set_wrap: true,
                        },
                    },
                },

                // Component cards
                append = &Box {
                    set_orientation: Orientation::Vertical,
                    set_spacing: 12,

                    // Vulkan
                    append = &Box {
                        set_orientation: Orientation::Horizontal,
                        set_spacing: 12,
                        set_hexpand: true,
                        set_css_classes: &["card", "setup-row"],

                        append = &Image {
                            set_icon_name: Some("video-display-symbolic"),
                            set_pixel_size: 24,
                        },

                        append = &Box {
                            set_orientation: Orientation::Vertical,
                            set_spacing: 4,
                            set_hexpand: true,

                            append = &Label {
                                set_label: "Vulkan Tools",
                                set_css_classes: &["card-title"],
                                set_halign: gtk4::Align::Start,
                            },

                            append = &Label {
                                set_label: "Required for DXVK and Vulkan games.",
                                set_css_classes: &["muted"],
                                set_halign: gtk4::Align::Start,
                                set_wrap: true,
                            },
                        },

                        append = &Box {
                            set_orientation: Orientation::Vertical,
                            set_spacing: 6,
                            set_halign: gtk4::Align::End,

                            append = &Label {
                                #[watch]
                                set_markup: if model.system_check.vulkan_installed {
                                    "<span foreground='#2ecc71'>✓ Installed</span>"
                                } else {
                                    "<span foreground='#e74c3c'>✗ Missing</span>"
                                },
                                #[watch]
                                set_css_classes: if model.system_check.vulkan_installed {
                                    &["pill", "pill-installed"]
                                } else {
                                    &["pill", "pill-missing"]
                                },
                                set_halign: gtk4::Align::End,
                            },

                            append = &Box {
                                set_orientation: Orientation::Horizontal,
                                set_spacing: 8,

                                append = &Button {
                                    #[watch]
                                    set_visible: !model.system_check.vulkan_installed,
                                    set_label: "Copy setup cmd",
                                    set_css_classes: &["secondary"],
                                    connect_clicked => SystemSetupMsg::CopySetupScript { reinstall: false },
                                },

                                append = &Button {
                                    #[watch]
                                    set_visible: model.system_check.vulkan_installed,
                                    set_label: "Copy reinstall cmd",
                                    set_css_classes: &["secondary"],
                                    connect_clicked => SystemSetupMsg::CopySetupScript { reinstall: true },
                                },
                            },
                        },
                    },

                    // Mesa
                    append = &Box {
                        set_orientation: Orientation::Horizontal,
                        set_spacing: 12,
                        set_hexpand: true,
                        set_css_classes: &["card", "setup-row"],

                        append = &Image {
                            set_icon_name: Some("drive-harddisk-symbolic"),
                            set_pixel_size: 24,
                        },

                        append = &Box {
                            set_orientation: Orientation::Vertical,
                            set_spacing: 4,
                            set_hexpand: true,

                            append = &Label {
                                set_label: "Mesa Drivers",
                                set_css_classes: &["card-title"],
                                set_halign: gtk4::Align::Start,
                            },

                            append = &Label {
                                set_label: "Open-source GPU drivers for Linux.",
                                set_css_classes: &["muted"],
                                set_halign: gtk4::Align::Start,
                                set_wrap: true,
                            },
                        },

                        append = &Box {
                            set_orientation: Orientation::Vertical,
                            set_spacing: 6,
                            set_halign: gtk4::Align::End,

                            append = &Label {
                                #[watch]
                                set_markup: if model.system_check.mesa_installed {
                                    "<span foreground='#2ecc71'>✓ Installed</span>"
                                } else {
                                    "<span foreground='#e74c3c'>✗ Missing</span>"
                                },
                                #[watch]
                                set_css_classes: if model.system_check.mesa_installed {
                                    &["pill", "pill-installed"]
                                } else {
                                    &["pill", "pill-missing"]
                                },
                                set_halign: gtk4::Align::End,
                            },

                            append = &Box {
                                set_orientation: Orientation::Horizontal,
                                set_spacing: 8,

                                append = &Button {
                                    #[watch]
                                    set_visible: !model.system_check.mesa_installed,
                                    set_label: "Copy setup cmd",
                                    set_css_classes: &["secondary"],
                                    connect_clicked => SystemSetupMsg::CopySetupScript { reinstall: false },
                                },

                                append = &Button {
                                    #[watch]
                                    set_visible: model.system_check.mesa_installed,
                                    set_label: "Copy reinstall cmd",
                                    set_css_classes: &["secondary"],
                                    connect_clicked => SystemSetupMsg::CopySetupScript { reinstall: true },
                                },
                            },
                        },
                    },

                    // UMU Launcher
                    append = &Box {
                        set_orientation: Orientation::Horizontal,
                        set_spacing: 12,
                        set_hexpand: true,
                        set_css_classes: &["card", "setup-row"],

                        append = &Image {
                            set_icon_name: Some("utilities-terminal-symbolic"),
                            set_pixel_size: 24,
                        },

                        append = &Box {
                            set_orientation: Orientation::Vertical,
                            set_spacing: 4,
                            set_hexpand: true,

                            append = &Label {
                                set_label: "UMU Launcher",
                                set_css_classes: &["card-title"],
                                set_halign: gtk4::Align::Start,
                            },

                            append = &Label {
                                set_label: "Required to run Proton-GE outside Steam.",
                                set_css_classes: &["muted"],
                                set_halign: gtk4::Align::Start,
                                set_wrap: true,
                            },
                        },

                        append = &Box {
                            set_orientation: Orientation::Vertical,
                            set_spacing: 6,
                            set_halign: gtk4::Align::End,

                            append = &Label {
                                #[watch]
                                set_markup: &model.umu_status_markup,
                                #[watch]
                                set_css_classes: if model.system_check.umu_installed {
                                    &["pill", "pill-installed"]
                                } else {
                                    &["pill", "pill-missing"]
                                },
                                set_halign: gtk4::Align::End,
                            },

                            append = &Box {
                                set_orientation: Orientation::Horizontal,
                                set_spacing: 8,

                                append = &Button {
                                    #[watch]
                                    set_visible: !model.system_check.umu_installed,
                                    set_label: "Copy setup cmd",
                                    set_css_classes: &["secondary"],
                                    connect_clicked => SystemSetupMsg::CopySetupScript { reinstall: false },
                                },

                                append = &Button {
                                    #[watch]
                                    set_visible: model.system_check.umu_installed,
                                    set_label: "Copy reinstall cmd",
                                    set_css_classes: &["secondary"],
                                    connect_clicked => SystemSetupMsg::CopySetupScript { reinstall: true },
                                },
                            },
                        },
                    },

                    // Proton-GE
                    append = &Box {
                        set_orientation: Orientation::Horizontal,
                        set_spacing: 12,
                        set_hexpand: true,
                        set_css_classes: &["card", "setup-row"],

                        append = &Image {
                            set_icon_name: Some("folder-download-symbolic"),
                            set_pixel_size: 24,
                        },

                        append = &Box {
                            set_orientation: Orientation::Vertical,
                            set_spacing: 4,
                            set_hexpand: true,

                            append = &Label {
                                set_label: "Proton-GE",
                                set_css_classes: &["card-title"],
                                set_halign: gtk4::Align::Start,
                            },

                            append = &Label {
                                set_label: "Download the latest runtime for gaming.",
                                set_css_classes: &["muted"],
                                set_halign: gtk4::Align::Start,
                                set_wrap: true,
                            },
                        },

                        append = &Box {
                            set_orientation: Orientation::Vertical,
                            set_spacing: 6,
                            set_halign: gtk4::Align::End,

                            append = &Label {
                                #[watch]
                                set_markup: &model.proton_status_markup,
                                #[watch]
                                set_css_classes: if model.system_check.proton_installed {
                                    &["pill", "pill-installed"]
                                } else {
                                    &["pill", "pill-warning"]
                                },
                                set_halign: gtk4::Align::End,
                            },

                            append = &Box {
                                set_orientation: Orientation::Horizontal,
                                set_spacing: 8,

                                append = &Button {
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
                                    connect_clicked => SystemSetupMsg::DownloadProton { reinstall: false },
                                },

                                append = &Button {
                                    #[watch]
                                    set_label: if model.is_downloading {
                                        "Reinstalling..."
                                    } else {
                                        "Reinstall Latest"
                                    },
                                    #[watch]
                                    set_visible: model.system_check.proton_installed,
                                    #[watch]
                                    set_sensitive: !model.is_downloading,
                                    connect_clicked => SystemSetupMsg::DownloadProton { reinstall: true },
                                },
                            },
                        },
                    },
                },

                // Missing APT packages section
                append = &Box {
                    set_orientation: Orientation::Vertical,
                    set_spacing: 8,
                    set_margin_top: 8,
                    set_css_classes: &["card"],
                    #[watch]
                    set_visible: !model.system_check.missing_apt_packages.is_empty(),

                    append = &Box {
                        set_orientation: Orientation::Horizontal,
                        set_spacing: 8,

                        append = &Image {
                            set_icon_name: Some("dialog-warning-symbolic"),
                            set_pixel_size: 20,
                        },

                        append = &Label {
                            set_label: "Missing System Packages",
                            set_css_classes: &["card-title"],
                            set_halign: gtk4::Align::Start,
                        },
                    },

                    append = &Label {
                        set_label: "Install the following packages to enable graphics support:",
                        set_css_classes: &["muted"],
                        set_halign: gtk4::Align::Start,
                        set_wrap: true,
                    },

                    append = &Label {
                        #[watch]
                        set_label: &model.system_check.missing_apt_packages.join(" "),
                        set_halign: gtk4::Align::Start,
                        set_selectable: true,
                        set_wrap: true,
                    },

                    append = &Button {
                        set_halign: gtk4::Align::Start,
                        set_label: "Copy setup command",
                        set_css_classes: &["secondary"],
                        connect_clicked => SystemSetupMsg::CopySetupScript { reinstall: false },
                    },
                },

                // Download status area
                append = &Box {
                    set_orientation: Orientation::Vertical,
                    set_spacing: 8,
                    set_margin_top: 8,
                    set_css_classes: &["card"],
                    #[watch]
                    set_visible: model.is_downloading || !model.download_status.is_empty(),

                    append = &Box {
                        set_orientation: Orientation::Horizontal,
                        set_spacing: 8,

                        append = &Image {
                            set_icon_name: Some("folder-download-symbolic"),
                            set_pixel_size: 20,
                        },

                        append = &Label {
                            set_label: "Download Status",
                            set_css_classes: &["card-title"],
                            set_halign: gtk4::Align::Start,
                        },
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
                        set_css_classes: &["muted"],
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

                // Bottom buttons
                append = &Box {
                    set_orientation: Orientation::Horizontal,
                    set_spacing: 10,
                    set_halign: gtk4::Align::End,

                    append = &Button {
                        set_label: "Refresh Status",
                        set_css_classes: &["secondary"],
                        connect_clicked => SystemSetupMsg::RefreshStatus,
                    },

                    append = &Button {
                        set_label: "Close",
                        set_css_classes: &["accent"],
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
        let proton_installed_version = runtime_mgr
            .list_installed()
            .ok()
            .and_then(|mut versions| {
                versions.sort();
                versions.last().cloned()
            });

        let umu_installed_version = if system_check.umu_installed {
            Self::detect_umu_version()
        } else {
            None
        };

        let mut model = SystemSetupDialog {
            system_check,
            runtime_mgr,
            download_status: String::new(),
            download_progress: 0.0,
            download_version: None,
            is_downloading: false,
            proton_installed_version,
            umu_installed_version,
            umu_status_markup: String::new(),
            proton_status_markup: String::new(),
        };

        model.update_status_markup();

        let widgets = view_output!();
        
        // Show the dialog
        root.present();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            SystemSetupMsg::DownloadProton { reinstall } => {
                println!("Starting Proton-GE download in background...");
                self.is_downloading = true;
                if reinstall {
                    self.download_status = "Preparing reinstall...".to_string();
                } else {
                    self.download_status = "Fetching latest release information...".to_string();
                }
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
                            match runtime_mgr.install_proton_ge(&release, reinstall, |status, progress| {
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
                self.proton_installed_version = self.download_version.clone();
                // Refresh system check
                self.system_check = SystemCheck::check();
                self.update_status_markup();
                let _ = sender.output(SystemSetupOutput::SystemCheckUpdated(
                    self.system_check.clone(),
                ));
            }
            
            SystemSetupMsg::DownloadError(error) => {
                self.is_downloading = false;
                self.download_status = format!("✗ Error: {}", error);
                self.download_progress = 0.0;
            }

            SystemSetupMsg::CopySetupScript { reinstall } => {
                let command = Self::setup_script_command(reinstall);
                Self::copy_to_clipboard(&command);
                println!("Copied to clipboard: {}", command);
            }

            SystemSetupMsg::RefreshStatus => {
                self.system_check = SystemCheck::check();
                if self.system_check.proton_installed {
                    self.proton_installed_version = self
                        .runtime_mgr
                        .list_installed()
                        .ok()
                        .and_then(|mut versions| {
                            versions.sort();
                            versions.last().cloned()
                        });
                } else {
                    self.proton_installed_version = None;
                }
                self.umu_installed_version = if self.system_check.umu_installed {
                    Self::detect_umu_version()
                } else {
                    None
                };
                self.update_status_markup();
                let _ = sender.output(SystemSetupOutput::SystemCheckUpdated(
                    self.system_check.clone(),
                ));
            }

            SystemSetupMsg::Refresh(system_check) => {
                self.system_check = system_check;
                if self.system_check.proton_installed {
                    self.proton_installed_version = self
                        .runtime_mgr
                        .list_installed()
                        .ok()
                        .and_then(|mut versions| {
                            versions.sort();
                            versions.last().cloned()
                        });
                } else {
                    self.proton_installed_version = None;
                }
                self.umu_installed_version = if self.system_check.umu_installed {
                    Self::detect_umu_version()
                } else {
                    None
                };
                self.update_status_markup();
            }
            
            SystemSetupMsg::Close => {
                // Dialog closes when button is clicked
                println!("Closing system setup dialog");
                let _ = sender.output(SystemSetupOutput::CloseRequested);
            }
        }
    }
}
