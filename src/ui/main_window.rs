use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box, Button, Dialog, Entry, FileChooserAction, FileChooserNative,
    FileFilter, Label, Orientation, ResponseType, ScrolledWindow,
};
use relm4::{Component, ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent};
use relm4::component::{ComponentController, Controller};

use crate::core::capsule::{
    Capsule, CapsuleMetadata, InstallState,
};
use crate::core::runtime_manager::RuntimeManager;
use crate::core::system_checker::{SystemCheck, SystemStatus};
use crate::ui::system_setup_dialog::{SystemSetupDialog, SystemSetupMsg, SystemSetupOutput};
use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::{fs, thread};

#[derive(Debug)]
pub enum MainWindowMsg {
    LoadCapsules,
    OpenInstaller,
    OpenSystemSetup,
    InstallerSelected(PathBuf),
    InstallerCancelled,
    GameNameConfirmed(String),
    InstallerFinished {
        capsule_dir: PathBuf,
        success: bool,
    },
    SelectExecutable {
        capsule_dir: PathBuf,
        exe_path: PathBuf,
    },
    EditGame(PathBuf),
    DeleteGame(PathBuf),
    ResumeInstall(PathBuf),
    KillInstall(PathBuf),
    SystemSetupOutput(SystemSetupOutput),
}

pub struct MainWindow {
    capsules: Vec<Capsule>,
    games_dir: PathBuf,
    system_check: SystemCheck,
    system_setup_dialog: Option<Controller<SystemSetupDialog>>,
    runtime_mgr: RuntimeManager,
    installer_dialog: Option<FileChooserNative>,
    name_dialog: Option<Dialog>,
    pending_installer_path: Option<PathBuf>,
    active_installs: HashMap<PathBuf, i32>,
    games_list: Box,
    root_window: ApplicationWindow,
}

impl MainWindow {
    fn sanitize_name(name: &str) -> String {
        name.trim()
            .replace(['/', '\\'], "_")
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
    }

    fn unique_game_dir(&self, base_name: &str) -> PathBuf {
        let base = self.games_dir.join(base_name);
        if !base.exists() {
            return base;
        }

        for idx in 1..1000 {
            let candidate = self.games_dir.join(format!("{}-{}", base_name, idx));
            if !candidate.exists() {
                return candidate;
            }
        }

        base
    }

    fn has_command(cmd: &str) -> bool {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn open_installer_dialog(&mut self, sender: ComponentSender<Self>) {
        if let Some(dialog) = &self.installer_dialog {
            dialog.show();
            return;
        }

        let dialog = FileChooserNative::builder()
            .title("Select Installer")
            .action(FileChooserAction::Open)
            .accept_label("Select")
            .cancel_label("Cancel")
            .transient_for(&self.root_window)
            .build();

        let filter = FileFilter::new();
        filter.add_suffix("exe");
        filter.add_suffix("msi");
        filter.set_name(Some("Windows installers (.exe, .msi)"));
        dialog.add_filter(&filter);

        let sender_clone = sender.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                if let Some(file) = dialog.file() {
                    if let Some(path) = file.path() {
                        sender_clone.input(MainWindowMsg::InstallerSelected(path));
                    } else {
                        sender_clone.input(MainWindowMsg::InstallerCancelled);
                    }
                } else {
                    sender_clone.input(MainWindowMsg::InstallerCancelled);
                }
            } else {
                sender_clone.input(MainWindowMsg::InstallerCancelled);
            }

            dialog.destroy();
        });

        dialog.show();
        self.installer_dialog = Some(dialog);
    }

    fn open_name_dialog(&mut self, sender: ComponentSender<Self>) {
        if self.name_dialog.is_some() {
            return;
        }

        let dialog = Dialog::builder()
            .title("Game Name")
            .modal(true)
            .transient_for(&self.root_window)
            .build();
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Create", ResponseType::Accept);

        let content = dialog.content_area();
        let entry = Entry::new();
        entry.set_placeholder_text(Some("Enter game name"));
        content.append(&entry);

        let sender_clone = sender.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                let name = entry.text().to_string();
                sender_clone.input(MainWindowMsg::GameNameConfirmed(name));
            } else {
                sender_clone.input(MainWindowMsg::InstallerCancelled);
            }

            dialog.close();
        });

        dialog.show();
        self.name_dialog = Some(dialog);
    }

    fn open_exe_dialog(&self, sender: ComponentSender<Self>, capsule_dir: PathBuf) {
        let dialog = FileChooserNative::builder()
            .title("Select Game Executable")
            .action(FileChooserAction::Open)
            .accept_label("Select")
            .cancel_label("Cancel")
            .transient_for(&self.root_window)
            .build();

        let filter = FileFilter::new();
        filter.add_suffix("exe");
        filter.set_name(Some("Windows executables (.exe)"));
        dialog.add_filter(&filter);

        let sender_clone = sender.clone();
        dialog.connect_response(move |dialog, response| {
            if response == ResponseType::Accept {
                if let Some(file) = dialog.file() {
                    if let Some(path) = file.path() {
                        sender_clone.input(MainWindowMsg::SelectExecutable {
                            capsule_dir: capsule_dir.clone(),
                            exe_path: path,
                        });
                    }
                }
            }

            dialog.destroy();
        });

        dialog.show();
    }

    fn start_installer(
        &mut self,
        sender: &ComponentSender<Self>,
        capsule_dir: PathBuf,
        mut metadata: CapsuleMetadata,
        installer_path: PathBuf,
    ) {
        if !Self::has_command("umu-run") {
            eprintln!("umu-run not found in PATH");
            return;
        }

        let proton_path = match self.runtime_mgr.latest_installed() {
            Ok(Some(path)) => path,
            Ok(None) => {
                eprintln!("No Proton-GE runtime installed");
                return;
            }
            Err(e) => {
                eprintln!("Failed to resolve Proton-GE runtime: {}", e);
                return;
            }
        };

        let home_path = capsule_dir.join(format!("{}.AppImage.home", metadata.name));
        let prefix_path = home_path.join("prefix");
        if let Err(e) = fs::create_dir_all(prefix_path.join("drive_c")) {
            eprintln!("Failed to create prefix: {}", e);
            return;
        }

        metadata.installer_path = Some(installer_path.to_string_lossy().to_string());
        metadata.install_state = InstallState::Installing;

        let capsule = Capsule {
            name: metadata.name.clone(),
            capsule_dir: capsule_dir.clone(),
            home_path,
            metadata: metadata.clone(),
        };

        if let Err(e) = capsule.save_metadata() {
            eprintln!("Failed to save metadata: {}", e);
            return;
        }

        let mut cmd = Command::new("umu-run");
        cmd.env("WINEPREFIX", &prefix_path);
        cmd.env("PROTONPATH", &proton_path);
        cmd.env("GAMEID", "umu-default");
        cmd.env("STORE", "none");
        cmd.arg(&installer_path);

        unsafe {
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                eprintln!("Failed to launch installer: {}", e);
                return;
            }
        };

        let pid = child.id() as i32;
        let pgid = unsafe { libc::getpgid(pid) };
        if pgid > 0 {
            self.active_installs.insert(capsule_dir.clone(), pgid);
        }

        let sender_clone = sender.clone();
        thread::spawn(move || {
            let success = child.wait().map(|status| status.success()).unwrap_or(false);
            let _ = sender_clone.input(MainWindowMsg::InstallerFinished {
                capsule_dir,
                success,
            });
        });
    }

    fn rebuild_games_list(&mut self, sender: ComponentSender<Self>) {
        let list = &self.games_list;
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }

        if self.capsules.is_empty() {
            let empty = Label::new(Some("No games installed yet."));
            empty.set_halign(gtk4::Align::Start);
            list.append(&empty);
            return;
        }

        for capsule in &self.capsules {
            let row = Box::new(Orientation::Horizontal, 12);
            row.set_margin_all(8);
            row.set_hexpand(true);

            let name = Label::new(Some(&capsule.name));
            name.set_halign(gtk4::Align::Start);
            name.set_hexpand(true);

            let status_text = match capsule.metadata.install_state {
                InstallState::Installing => "Installing",
                InstallState::Installed => "Installed",
            };
            let status = Label::new(Some(status_text));
            status.set_halign(gtk4::Align::Start);

            let actions = Box::new(Orientation::Horizontal, 8);

            let edit_dir = capsule.capsule_dir.clone();
            let edit_sender = sender.clone();
            let edit_button = Button::with_label("Edit");
            edit_button.connect_clicked(move |_| {
                edit_sender.input(MainWindowMsg::EditGame(edit_dir.clone()));
            });
            actions.append(&edit_button);

            let delete_dir = capsule.capsule_dir.clone();
            let delete_sender = sender.clone();
            let delete_button = Button::with_label("Delete");
            delete_button.connect_clicked(move |_| {
                delete_sender.input(MainWindowMsg::DeleteGame(delete_dir.clone()));
            });
            actions.append(&delete_button);

            let installing = capsule.metadata.install_state == InstallState::Installing;
            let is_running = self.active_installs.contains_key(&capsule.capsule_dir);

            if installing && is_running {
                let kill_dir = capsule.capsule_dir.clone();
                let kill_sender = sender.clone();
                let kill_button = Button::with_label("Kill installer");
                kill_button.connect_clicked(move |_| {
                    kill_sender.input(MainWindowMsg::KillInstall(kill_dir.clone()));
                });
                actions.append(&kill_button);
            } else if installing {
                let resume_dir = capsule.capsule_dir.clone();
                let resume_sender = sender.clone();
                let resume_button = Button::with_label("Resume setup");
                resume_button.connect_clicked(move |_| {
                    resume_sender.input(MainWindowMsg::ResumeInstall(resume_dir.clone()));
                });
                actions.append(&resume_button);
            }

            row.append(&name);
            row.append(&status);
            row.append(&actions);
            list.append(&row);
        }
    }
}

#[allow(unused_assignments)]
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
                    #[local_ref]
                    set_child = &games_list -> Box {},
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

        let games_list = Box::new(Orientation::Vertical, 12);
        games_list.set_margin_all(20);
        games_list.set_valign(gtk4::Align::Start);

        let model = MainWindow {
            capsules: Vec::new(),
            games_dir,
            system_check,
            system_setup_dialog: None,
            runtime_mgr: RuntimeManager::new(),
            installer_dialog: None,
            name_dialog: None,
            pending_installer_path: None,
            active_installs: HashMap::new(),
            games_list: games_list.clone(),
            root_window: root.clone(),
        };

        let widgets = view_output!();

        // Load capsules on startup
        sender.input(MainWindowMsg::LoadCapsules);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            MainWindowMsg::LoadCapsules => {
                match Capsule::scan_directory(&self.games_dir) {
                    Ok(capsules) => {
                        self.capsules = capsules;
                        println!("Loaded {} capsules", self.capsules.len());
                        self.rebuild_games_list(sender.clone());
                    }
                    Err(e) => {
                        eprintln!("Failed to load capsules: {}", e);
                    }
                }
            }
            MainWindowMsg::OpenInstaller => {
                println!("Open installer dialog");
                self.open_installer_dialog(sender);
            }
            MainWindowMsg::InstallerSelected(path) => {
                self.installer_dialog = None;
                self.pending_installer_path = Some(path);
                self.open_name_dialog(sender);
            }
            MainWindowMsg::InstallerCancelled => {
                self.installer_dialog = None;
                self.name_dialog = None;
                self.pending_installer_path = None;
                println!("Installer selection cancelled");
            }
            MainWindowMsg::GameNameConfirmed(name) => {
                self.name_dialog = None;
                let installer_path = match self.pending_installer_path.take() {
                    Some(path) => path,
                    None => {
                        eprintln!("No installer path selected");
                        return;
                    }
                };

                let name = Self::sanitize_name(&name);
                if name.is_empty() {
                    eprintln!("Game name cannot be empty");
                    return;
                }

                if let Err(e) = fs::create_dir_all(&self.games_dir) {
                    eprintln!("Failed to create games directory: {}", e);
                    return;
                }

                let capsule_dir = self.unique_game_dir(&name);
                if let Err(e) = fs::create_dir_all(&capsule_dir) {
                    eprintln!("Failed to create capsule directory: {}", e);
                    return;
                }

                let mut metadata = CapsuleMetadata::default();
                metadata.name = name.clone();
                metadata.installer_path = Some(installer_path.to_string_lossy().to_string());
                metadata.install_state = InstallState::Installing;

                self.start_installer(&sender, capsule_dir, metadata, installer_path);
                sender.input(MainWindowMsg::LoadCapsules);
            }
            MainWindowMsg::InstallerFinished { capsule_dir, success } => {
                self.active_installs.remove(&capsule_dir);
                if success {
                    println!("Installer completed for {:?}", capsule_dir);
                } else {
                    eprintln!("Installer failed for {:?}", capsule_dir);
                }
                sender.input(MainWindowMsg::LoadCapsules);
            }
            MainWindowMsg::EditGame(capsule_dir) => {
                self.open_exe_dialog(sender, capsule_dir);
            }
            MainWindowMsg::SelectExecutable { capsule_dir, exe_path } => {
                match Capsule::load_from_dir(&capsule_dir) {
                    Ok(mut capsule) => {
                        capsule.metadata.executables.main.path =
                            exe_path.to_string_lossy().to_string();
                        capsule.metadata.install_state = InstallState::Installed;
                        if let Err(e) = capsule.save_metadata() {
                            eprintln!("Failed to update metadata: {}", e);
                        } else {
                            println!("Updated executable for {}", capsule.name);
                            sender.input(MainWindowMsg::LoadCapsules);
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to load capsule: {}", e);
                    }
                }
            }
            MainWindowMsg::DeleteGame(capsule_dir) => {
                if let Err(e) = fs::remove_dir_all(&capsule_dir) {
                    eprintln!("Failed to delete capsule: {}", e);
                } else {
                    println!("Deleted capsule {:?}", capsule_dir);
                    sender.input(MainWindowMsg::LoadCapsules);
                }
            }
            MainWindowMsg::ResumeInstall(capsule_dir) => {
                match Capsule::load_from_dir(&capsule_dir) {
                    Ok(capsule) => {
                        let installer_path = capsule
                            .metadata
                            .installer_path
                            .as_ref()
                            .map(PathBuf::from);
                        if let Some(installer_path) = installer_path {
                            self.start_installer(
                                &sender,
                                capsule_dir,
                                capsule.metadata.clone(),
                                installer_path,
                            );
                            self.rebuild_games_list(sender.clone());
                        } else {
                            eprintln!("No installer path found for {}", capsule.name);
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to load capsule: {}", e);
                    }
                }
            }
            MainWindowMsg::KillInstall(capsule_dir) => {
                if let Some(pgid) = self.active_installs.remove(&capsule_dir) {
                    unsafe {
                        libc::kill(-pgid, libc::SIGKILL);
                    }
                    println!("Killed installer for {:?}", capsule_dir);
                    self.rebuild_games_list(sender.clone());
                }
            }
            MainWindowMsg::OpenSystemSetup => {
                // Re-check system status before opening dialog
                self.system_check = SystemCheck::check();
                
                println!("Opening system setup dialog...");
                
                if let Some(dialog) = &self.system_setup_dialog {
                    dialog.emit(SystemSetupMsg::Refresh(self.system_check.clone()));
                    dialog.widget().present();
                } else {
                    let dialog = SystemSetupDialog::builder()
                        .launch(self.system_check.clone())
                        .forward(sender.input_sender(), MainWindowMsg::SystemSetupOutput);
                    dialog.widget().present();
                    self.system_setup_dialog = Some(dialog);
                }
            }
            MainWindowMsg::SystemSetupOutput(SystemSetupOutput::CloseRequested) => {
                if let Some(dialog) = &self.system_setup_dialog {
                    dialog.widget().close();
                }
            }
            MainWindowMsg::SystemSetupOutput(SystemSetupOutput::SystemCheckUpdated(system_check)) => {
                self.system_check = system_check;
            }
        }
    }

}
