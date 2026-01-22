use gtk4::prelude::*;
use gtk4::{Dialog, Box, Label, Button, Orientation, Grid, Separator, Frame, ProgressBar, ScrolledWindow};
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent};
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::core::system_checker::SystemCheck;
use crate::core::runtime_manager::RuntimeManager;

#[derive(Debug)]
pub enum SystemSetupMsg {
    DownloadProton { reinstall: bool },
    DownloadProgress { status: String, progress: f64 },  // status text and 0.0-1.0 progress
    DownloadVersion(String),
    DownloadComplete,
    DownloadError(String),
    RunAptInstall { target: InstallTarget, reinstall: bool },
    RunUmuInstall { reinstall: bool },
    InstallLog(String),
    InstallFinished { success: bool, message: String },
    InstallVersion { kind: InstallKind, version: String },
    RefreshStatus,
    Refresh(SystemCheck),
    Close,
}

#[derive(Debug)]
pub enum SystemSetupOutput {
    CloseRequested,
    SystemCheckUpdated(SystemCheck),
}

#[derive(Debug, Clone, Copy)]
pub enum InstallTarget {
    Vulkan,
    Mesa,
    MissingPackages,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InstallKind {
    Apt(InstallTarget),
    Umu,
}

pub struct SystemSetupDialog {
    system_check: SystemCheck,
    runtime_mgr: RuntimeManager,
    download_status: String,
    download_progress: f64,  // 0.0 to 1.0
    download_version: Option<String>,
    is_downloading: bool,
    install_status: String,
    install_log: String,
    install_running: bool,
    install_kind: Option<InstallKind>,
    proton_installed_version: Option<String>,
    umu_installed_version: Option<String>,
    pending_umu_version: Option<String>,
}

impl SystemSetupDialog {
    const UMU_RELEASE_API: &'static str =
        "https://api.github.com/repos/Open-Wine-Components/umu-launcher/releases/latest";

    fn vulkan_packages() -> [&'static str; 3] {
        ["vulkan-tools", "libvulkan1", "libvulkan1:i386"]
    }

    fn mesa_packages() -> [&'static str; 6] {
        [
            "mesa-vulkan-drivers",
            "mesa-vulkan-drivers:i386",
            "libgl1-mesa-dri:amd64",
            "libgl1-mesa-dri:i386",
            "libglx-mesa0:amd64",
            "libglx-mesa0:i386",
        ]
    }

    fn parse_os_release() -> Option<std::collections::HashMap<String, String>> {
        let contents = std::fs::read_to_string("/etc/os-release").ok()?;
        let mut map = std::collections::HashMap::new();
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let cleaned = value.trim().trim_matches('"').to_string();
                map.insert(key.to_string(), cleaned);
            }
        }
        Some(map)
    }

    fn deb_arch() -> Option<&'static str> {
        match std::env::consts::ARCH {
            "x86_64" => Some("amd64"),
            "aarch64" => Some("arm64"),
            "arm" => Some("armhf"),
            _ => None,
        }
    }

    fn resolve_command(name: &str, fallback_paths: &[&str]) -> Option<String> {
        for path in fallback_paths {
            if Path::new(path).exists() {
                return Some(path.to_string());
            }
        }
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) {
                let candidate = dir.join(name);
                if candidate.exists() {
                    return Some(candidate.to_string_lossy().to_string());
                }
            }
        }
        None
    }

    fn select_umu_asset(assets: &[UmuAsset], debian_version: &str, arch: &str) -> Option<UmuAsset> {
        let exact = assets.iter().find(|asset| {
            asset.name.ends_with(".deb")
                && asset.name.contains("python3-umu-launcher")
                && asset.name.contains(&format!("_{}_debian-{}", arch, debian_version))
        });
        if let Some(asset) = exact {
            return Some(asset.clone());
        }

        let fallback = assets.iter().find(|asset| {
            asset.name.ends_with(".deb")
                && asset.name.contains("umu-launcher")
                && asset.name.contains(&format!("_all_debian-{}", debian_version))
        });
        fallback.cloned()
    }

    fn fetch_latest_umu_release() -> Result<UmuRelease, String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("LinuxBoy/0.1")
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
        let response = client
            .get(Self::UMU_RELEASE_API)
            .send()
            .map_err(|e| format!("Failed to fetch UMU release: {}", e))?;
        if !response.status().is_success() {
            return Err(format!(
                "UMU release fetch failed: HTTP {}",
                response.status()
            ));
        }
        response
            .json::<UmuRelease>()
            .map_err(|e| format!("Failed to parse UMU release JSON: {}", e))
    }

    fn spawn_command_with_logs(
        mut cmd: Command,
        tx: std::sync::mpsc::Sender<InstallUpdate>,
    ) -> Result<(), String> {
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to launch installer: {}", e))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to capture stdout".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "Failed to capture stderr".to_string())?;

        let tx_out = tx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                let _ = tx_out.send(InstallUpdate::Log(line));
            }
        });

        let tx_err = tx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().flatten() {
                let _ = tx_err.send(InstallUpdate::Log(line));
            }
        });

        let status = child
            .wait()
            .map_err(|e| format!("Install process failed: {}", e))?;
        let success = status.success();
        let _ = tx.send(InstallUpdate::Finished {
            success,
            message: if success {
                "Install completed successfully.".to_string()
            } else {
                format!("Install failed with status: {}", status)
            },
        });
        Ok(())
    }

}

#[derive(Debug, Deserialize, Clone)]
struct UmuRelease {
    pub tag_name: String,
    pub assets: Vec<UmuAsset>,
}

#[derive(Debug, Deserialize, Clone)]
struct UmuAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

#[derive(Debug)]
enum InstallUpdate {
    Log(String),
    Version { kind: InstallKind, version: String },
    Finished { success: bool, message: String },
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
                    attach[2, 1, 1, 1] = &Box {
                        set_orientation: Orientation::Horizontal,
                        set_spacing: 8,

                        append = &Button {
                            #[watch]
                            set_visible: !model.system_check.vulkan_installed,
                            set_label: "Install",
                            #[watch]
                            set_sensitive: !model.install_running,
                            connect_clicked => SystemSetupMsg::RunAptInstall {
                                target: InstallTarget::Vulkan,
                                reinstall: false,
                            },
                        },

                        append = &Button {
                            #[watch]
                            set_visible: model.system_check.vulkan_installed,
                            set_label: "Reinstall",
                            #[watch]
                            set_sensitive: !model.install_running,
                            connect_clicked => SystemSetupMsg::RunAptInstall {
                                target: InstallTarget::Vulkan,
                                reinstall: true,
                            },
                        },
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
                    attach[2, 2, 1, 1] = &Box {
                        set_orientation: Orientation::Horizontal,
                        set_spacing: 8,

                        append = &Button {
                            #[watch]
                            set_visible: !model.system_check.mesa_installed,
                            set_label: "Install",
                            #[watch]
                            set_sensitive: !model.install_running,
                            connect_clicked => SystemSetupMsg::RunAptInstall {
                                target: InstallTarget::Mesa,
                                reinstall: false,
                            },
                        },

                        append = &Button {
                            #[watch]
                            set_visible: model.system_check.mesa_installed,
                            set_label: "Reinstall",
                            #[watch]
                            set_sensitive: !model.install_running,
                            connect_clicked => SystemSetupMsg::RunAptInstall {
                                target: InstallTarget::Mesa,
                                reinstall: true,
                            },
                        },
                    },

                    // Row 3: UMU Launcher
                    attach[0, 3, 1, 1] = &Label {
                        set_label: "UMU Launcher",
                        set_halign: gtk4::Align::Start,
                    },
                    attach[1, 3, 1, 1] = &Label {
                        #[watch]
                        set_markup: if model.system_check.umu_installed {
                            if let Some(version) = &model.umu_installed_version {
                                format!(
                                    "<span foreground='#2ecc71'>✓ Installed ({})</span>",
                                    version
                                )
                            } else {
                                "<span foreground='#2ecc71'>✓ Installed</span>".to_string()
                            }
                        } else {
                            "<span foreground='#e74c3c'>✗ Missing</span>".to_string()
                        },
                        set_halign: gtk4::Align::Start,
                    },
                    attach[2, 3, 1, 1] = &Box {
                        set_orientation: Orientation::Horizontal,
                        set_spacing: 10,

                        append = &Button {
                            #[watch]
                            set_visible: !model.system_check.umu_installed,
                            set_label: "Install",
                            #[watch]
                            set_sensitive: !model.install_running,
                            connect_clicked => SystemSetupMsg::RunUmuInstall { reinstall: false },
                        },

                        append = &Button {
                            #[watch]
                            set_visible: model.system_check.umu_installed,
                            set_label: "Reinstall",
                            #[watch]
                            set_sensitive: !model.install_running,
                            connect_clicked => SystemSetupMsg::RunUmuInstall { reinstall: true },
                        },
                    },

                    // Row 4: Proton-GE
                    attach[0, 4, 1, 1] = &Label {
                        set_label: "Proton-GE",
                        set_halign: gtk4::Align::Start,
                    },
                    attach[1, 4, 1, 1] = &Label {
                        #[watch]
                        set_markup: if model.system_check.proton_installed {
                            if let Some(version) = &model.proton_installed_version {
                                format!(
                                    "<span foreground='#2ecc71'>✓ Installed ({})</span>",
                                    version
                                )
                            } else {
                                "<span foreground='#2ecc71'>✓ Installed</span>".to_string()
                            }
                        } else {
                            "<span foreground='#f39c12'>✗ Not Downloaded</span>".to_string()
                        },
                        set_halign: gtk4::Align::Start,
                    },
                    attach[2, 4, 1, 1] = &Box {
                        set_orientation: Orientation::Horizontal,
                        set_spacing: 10,

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
                        set_label: "Install the following packages to enable graphics support:",
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
                                set_label: &model.system_check.missing_apt_packages.join(" "),
                                set_halign: gtk4::Align::Start,
                                set_selectable: true,
                                set_wrap: true,
                            },
                        },
                    },

                    append = &Button {
                        set_halign: gtk4::Align::Start,
                        set_label: "Install missing packages",
                        #[watch]
                        set_sensitive: !model.install_running,
                        connect_clicked => SystemSetupMsg::RunAptInstall {
                            target: InstallTarget::MissingPackages,
                            reinstall: false,
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

                // Install output area
                append = &Box {
                    set_orientation: Orientation::Vertical,
                    set_spacing: 10,
                    set_margin_top: 10,
                    #[watch]
                    set_visible: model.install_running || !model.install_log.is_empty(),

                    append = &Separator {
                        set_orientation: Orientation::Horizontal,
                    },

                    append = &Label {
                        set_markup: "<b>Install Output</b>",
                        set_halign: gtk4::Align::Start,
                    },

                    append = &Label {
                        #[watch]
                        set_label: &model.install_status,
                        set_halign: gtk4::Align::Start,
                        set_wrap: true,
                    },

                    append = &ScrolledWindow {
                        set_min_content_height: 160,
                        #[wrap(Some)]
                        set_child = &Label {
                            #[watch]
                            set_label: &model.install_log,
                            set_halign: gtk4::Align::Start,
                            set_selectable: true,
                            set_wrap: true,
                        },
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
                        set_label: "Refresh Status",
                        connect_clicked => SystemSetupMsg::RefreshStatus,
                    },

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
        let proton_installed_version = runtime_mgr
            .list_installed()
            .ok()
            .and_then(|mut versions| {
                versions.sort();
                versions.last().cloned()
            });

        let model = SystemSetupDialog {
            system_check,
            runtime_mgr,
            download_status: String::new(),
            download_progress: 0.0,
            download_version: None,
            is_downloading: false,
            install_status: String::new(),
            install_log: String::new(),
            install_running: false,
            install_kind: None,
            proton_installed_version,
            umu_installed_version: None,
            pending_umu_version: None,
        };

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
                let _ = sender.output(SystemSetupOutput::SystemCheckUpdated(
                    self.system_check.clone(),
                ));
            }
            
            SystemSetupMsg::DownloadError(error) => {
                self.is_downloading = false;
                self.download_status = format!("✗ Error: {}", error);
                self.download_progress = 0.0;
            }

            SystemSetupMsg::RunAptInstall { target, reinstall } => {
                if self.install_running {
                    return;
                }

                self.install_kind = Some(InstallKind::Apt(target));
                let pkexec_path = match Self::resolve_command("pkexec", &["/usr/bin/pkexec", "/bin/pkexec"]) {
                    Some(path) => path,
                    None => {
                        self.install_status = "Install failed.".to_string();
                        self.install_log = "pkexec not found. Install policykit-1 to enable GUI installs.".to_string();
                        return;
                    }
                };
                let apt_path = match Self::resolve_command(
                    "apt",
                    &["/usr/bin/apt", "/bin/apt", "/usr/bin/apt-get", "/bin/apt-get"],
                ) {
                    Some(path) => path,
                    None => {
                        self.install_status = "Install failed.".to_string();
                        self.install_log = "apt not found in PATH.".to_string();
                        return;
                    }
                };
                let packages: Vec<String> = match target {
                    InstallTarget::Vulkan => Self::vulkan_packages().iter().map(|s| s.to_string()).collect(),
                    InstallTarget::Mesa => Self::mesa_packages().iter().map(|s| s.to_string()).collect(),
                    InstallTarget::MissingPackages => {
                        if self.system_check.missing_apt_packages.is_empty() {
                            self.install_status = "No missing packages to install.".to_string();
                            return;
                        }
                        self.system_check.missing_apt_packages.clone()
                    }
                };

                self.install_running = true;
                self.install_log.clear();
                self.install_status = match target {
                    InstallTarget::Vulkan => "Installing Vulkan packages...".to_string(),
                    InstallTarget::Mesa => "Installing Mesa packages...".to_string(),
                    InstallTarget::MissingPackages => "Installing missing packages...".to_string(),
                };

                let sender_clone = sender.clone();
                let (tx, rx) = std::sync::mpsc::channel::<InstallUpdate>();

                std::thread::spawn(move || {
                    let mut cmd = Command::new(pkexec_path);
                    cmd.arg("env")
                        .arg("DEBIAN_FRONTEND=noninteractive")
                        .arg(apt_path)
                        .arg("install")
                        .arg("-y");
                    if reinstall {
                        cmd.arg("--reinstall");
                    }
                    for pkg in packages {
                        cmd.arg(pkg);
                    }

                    if let Err(err) = Self::spawn_command_with_logs(cmd, tx.clone()) {
                        let _ = tx.send(InstallUpdate::Finished {
                            success: false,
                            message: err,
                        });
                    }
                });

                glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                    let mut finished = None;
                    while let Ok(update) = rx.try_recv() {
                        match update {
                            InstallUpdate::Log(line) => {
                                let _ = sender_clone.input(SystemSetupMsg::InstallLog(line));
                            }
                            InstallUpdate::Version { kind, version } => {
                                let _ = sender_clone.input(SystemSetupMsg::InstallVersion { kind, version });
                            }
                            InstallUpdate::Finished { success, message } => {
                                finished = Some((success, message));
                            }
                        }
                    }

                    if let Some((success, message)) = finished {
                        let _ = sender_clone.input(SystemSetupMsg::InstallFinished {
                            success,
                            message,
                        });
                        return glib::ControlFlow::Break;
                    }

                    glib::ControlFlow::Continue
                });
            }

            SystemSetupMsg::RunUmuInstall { reinstall } => {
                if self.install_running {
                    return;
                }

                self.install_kind = Some(InstallKind::Umu);
                let pkexec_path = match Self::resolve_command("pkexec", &["/usr/bin/pkexec", "/bin/pkexec"]) {
                    Some(path) => path,
                    None => {
                        self.install_status = "Install failed.".to_string();
                        self.install_log = "pkexec not found. Install policykit-1 to enable GUI installs.".to_string();
                        return;
                    }
                };
                let apt_path = match Self::resolve_command(
                    "apt",
                    &["/usr/bin/apt", "/bin/apt", "/usr/bin/apt-get", "/bin/apt-get"],
                ) {
                    Some(path) => path,
                    None => {
                        self.install_status = "Install failed.".to_string();
                        self.install_log = "apt not found in PATH.".to_string();
                        return;
                    }
                };
                self.install_running = true;
                self.install_log.clear();
                self.install_status = if reinstall {
                    "Reinstalling UMU Launcher...".to_string()
                } else {
                    "Installing UMU Launcher...".to_string()
                };

                let runtime_mgr = self.runtime_mgr.clone();
                let sender_clone = sender.clone();
                let (tx, rx) = std::sync::mpsc::channel::<InstallUpdate>();

                std::thread::spawn(move || {
                    let os_release = Self::parse_os_release()
                        .ok_or_else(|| "Unable to read /etc/os-release".to_string());
                    let arch = Self::deb_arch()
                        .ok_or_else(|| "Unsupported CPU architecture".to_string());

                    let result: Result<(), String> = (|| {
                        let os_release = os_release?;
                        let distro_id = os_release
                            .get("ID")
                            .cloned()
                            .unwrap_or_else(|| "unknown".to_string());
                        let version_id = os_release
                            .get("VERSION_ID")
                            .cloned()
                            .unwrap_or_else(|| "unknown".to_string());

                        if distro_id != "debian" {
                            return Err(format!(
                                "Unsupported distro for .deb install: {}",
                                distro_id
                            ));
                        }

                        let arch = arch?;
                        let _ = tx.send(InstallUpdate::Log(format!(
                            "Detected distro: debian {} ({})",
                            version_id, arch
                        )));

                        let release = Self::fetch_latest_umu_release()?;
                        let _ = tx.send(InstallUpdate::Log(format!(
                            "Latest release: {}",
                            release.tag_name
                        )));
                        let _ = tx.send(InstallUpdate::Version {
                            kind: InstallKind::Umu,
                            version: release.tag_name.clone(),
                        });
                        let asset = Self::select_umu_asset(&release.assets, &version_id, arch)
                            .ok_or_else(|| "No matching .deb asset found for this distro/arch".to_string())?;

                        let _ = tx.send(InstallUpdate::Log(format!(
                            "Selected asset: {}",
                            asset.name
                        )));

                        let download_dir = std::env::temp_dir()
                            .join("linuxboy")
                            .join("downloads");
                        std::fs::create_dir_all(&download_dir)
                            .map_err(|e| format!("Failed to create download dir: {}", e))?;
                        let dest_path = download_dir.join(&asset.name);

                        let mut last_percent = 0u64;
                        runtime_mgr
                            .download_file(
                                &asset.browser_download_url,
                                &dest_path,
                                Some(asset.size),
                                |downloaded, total| {
                                    if total > 0 {
                                        let percent = (downloaded.saturating_mul(100)) / total;
                                        if percent >= last_percent + 5 || percent == 100 {
                                            last_percent = percent;
                                            let _ = tx.send(InstallUpdate::Log(format!(
                                                "Download {}% ({}/{})",
                                                percent, downloaded, total
                                            )));
                                        }
                                    }
                                },
                            )
                            .map_err(|e| format!("Download failed: {}", e))?;

                        let _ = tx.send(InstallUpdate::Log(
                            "Download complete. Installing package...".to_string(),
                        ));

                        let mut cmd = Command::new(pkexec_path);
                        cmd.arg("env")
                            .arg("DEBIAN_FRONTEND=noninteractive")
                            .arg(apt_path)
                            .arg("install")
                            .arg("-y");
                        if reinstall {
                            cmd.arg("--reinstall");
                        }
                        cmd.arg(dest_path);

                        Self::spawn_command_with_logs(cmd, tx.clone())?;
                        Ok(())
                    })();

                    if let Err(err) = result {
                        let _ = tx.send(InstallUpdate::Finished {
                            success: false,
                            message: err,
                        });
                    }
                });

                glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                    let mut finished = None;
                    while let Ok(update) = rx.try_recv() {
                        match update {
                            InstallUpdate::Log(line) => {
                                let _ = sender_clone.input(SystemSetupMsg::InstallLog(line));
                            }
                            InstallUpdate::Version { kind, version } => {
                                let _ = sender_clone.input(SystemSetupMsg::InstallVersion { kind, version });
                            }
                            InstallUpdate::Finished { success, message } => {
                                finished = Some((success, message));
                            }
                        }
                    }

                    if let Some((success, message)) = finished {
                        let _ = sender_clone.input(SystemSetupMsg::InstallFinished {
                            success,
                            message,
                        });
                        return glib::ControlFlow::Break;
                    }

                    glib::ControlFlow::Continue
                });
            }

            SystemSetupMsg::InstallLog(line) => {
                if !self.install_log.is_empty() {
                    self.install_log.push('\n');
                }
                self.install_log.push_str(&line);
            }

            SystemSetupMsg::InstallFinished { success, message } => {
                self.install_running = false;
                let completed_kind = self.install_kind.take();
                self.install_status = if success {
                    "Install completed.".to_string()
                } else {
                    "Install failed.".to_string()
                };
                if !message.is_empty() {
                    if !self.install_log.is_empty() {
                        self.install_log.push('\n');
                    }
                    self.install_log.push_str(&message);
                }
                if success && completed_kind == Some(InstallKind::Umu) {
                    if let Some(version) = self.pending_umu_version.take() {
                        self.umu_installed_version = Some(version);
                    }
                } else if !success {
                    self.pending_umu_version = None;
                }
                self.system_check = SystemCheck::check();
                let _ = sender.output(SystemSetupOutput::SystemCheckUpdated(
                    self.system_check.clone(),
                ));
            }

            SystemSetupMsg::InstallVersion { kind, version } => {
                if kind == InstallKind::Umu {
                    self.pending_umu_version = Some(version);
                }
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
                if !self.system_check.umu_installed {
                    self.umu_installed_version = None;
                }
                let _ = sender.output(SystemSetupOutput::SystemCheckUpdated(
                    self.system_check.clone(),
                ));
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
