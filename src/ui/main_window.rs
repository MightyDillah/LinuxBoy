use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box, Button, Dialog, Entry, FileChooserAction, FileChooserNative,
    FileFilter, Image, Label, Orientation, ResponseType, ScrolledWindow, Stack, StackSwitcher,
    StackTransitionType,
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
    library_count_label: Label,
    system_status_label: Label,
    system_status_detail: Label,
    root_window: ApplicationWindow,
}

impl MainWindow {
    fn update_library_labels(&self) {
        self.library_count_label
            .set_label(&format!("{} games", self.capsules.len()));
    }

    fn update_system_labels(&self) {
        let (title, class) = match self.system_check.status {
            SystemStatus::AllInstalled => ("System Ready", "status-ready"),
            SystemStatus::PartiallyInstalled => ("Setup Incomplete", "status-warning"),
            SystemStatus::NothingInstalled => ("Setup Required", "status-missing"),
        };

        self.system_status_label.set_label(title);
        self.system_status_label
            .set_css_classes(&["status-label", class]);
        self.system_status_detail
            .set_label(&self.system_check.status_message());
    }

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
            let empty = Box::new(Orientation::Horizontal, 12);
            empty.set_margin_all(8);
            empty.set_css_classes(&["card"]);

            let icon = Image::from_icon_name("applications-games-symbolic");
            icon.set_pixel_size(28);
            icon.set_halign(gtk4::Align::Start);
            icon.set_valign(gtk4::Align::Start);

            let text = Box::new(Orientation::Vertical, 6);
            text.set_hexpand(true);

            let title = Label::new(Some("No games yet"));
            title.set_css_classes(&["card-title"]);
            title.set_halign(gtk4::Align::Start);

            let subtitle = Label::new(Some(
                "Add an installer to create your first portable capsule.",
            ));
            subtitle.set_css_classes(&["muted"]);
            subtitle.set_halign(gtk4::Align::Start);
            subtitle.set_wrap(true);

            text.append(&title);
            text.append(&subtitle);

            empty.append(&icon);
            empty.append(&text);
            list.append(&empty);
            return;
        }

        for capsule in &self.capsules {
            let card = Box::new(Orientation::Vertical, 8);
            card.set_margin_bottom(12);
            card.set_hexpand(true);
            card.set_css_classes(&["card"]);

            let header = Box::new(Orientation::Horizontal, 10);
            header.set_hexpand(true);

            let icon = Image::from_icon_name("applications-games-symbolic");
            icon.set_pixel_size(24);
            icon.set_halign(gtk4::Align::Start);

            let name = Label::new(Some(&capsule.name));
            name.set_halign(gtk4::Align::Start);
            name.set_hexpand(true);
            name.set_css_classes(&["card-title"]);

            let status_text = match capsule.metadata.install_state {
                InstallState::Installing => "Installing",
                InstallState::Installed => "Installed",
            };
            let status_class = match capsule.metadata.install_state {
                InstallState::Installing => "pill-warning",
                InstallState::Installed => "pill-installed",
            };
            let status = Label::new(Some(status_text));
            status.set_css_classes(&["pill", status_class]);

            let spacer = Box::new(Orientation::Horizontal, 0);
            spacer.set_hexpand(true);

            header.append(&icon);
            header.append(&name);
            header.append(&spacer);
            header.append(&status);

            let installing = capsule.metadata.install_state == InstallState::Installing;
            let is_running = self.active_installs.contains_key(&capsule.capsule_dir);
            let detail_text = if installing {
                if is_running {
                    "Installer running"
                } else {
                    "Installer paused"
                }
            } else {
                "Ready to play"
            };

            let detail = Label::new(Some(detail_text));
            detail.set_css_classes(&["muted"]);
            detail.set_halign(gtk4::Align::Start);
            detail.set_margin_top(2);

            let actions = Box::new(Orientation::Horizontal, 8);
            actions.set_halign(gtk4::Align::Start);

            let edit_dir = capsule.capsule_dir.clone();
            let edit_sender = sender.clone();
            let edit_button = Button::with_label("Edit");
            edit_button.add_css_class("flat");
            edit_button.connect_clicked(move |_| {
                edit_sender.input(MainWindowMsg::EditGame(edit_dir.clone()));
            });
            actions.append(&edit_button);

            let delete_dir = capsule.capsule_dir.clone();
            let delete_sender = sender.clone();
            let delete_button = Button::with_label("Delete");
            delete_button.add_css_class("destructive-action");
            delete_button.connect_clicked(move |_| {
                delete_sender.input(MainWindowMsg::DeleteGame(delete_dir.clone()));
            });
            actions.append(&delete_button);

            if installing && is_running {
                let kill_dir = capsule.capsule_dir.clone();
                let kill_sender = sender.clone();
                let kill_button = Button::with_label("Kill installer");
                kill_button.add_css_class("destructive-action");
                kill_button.connect_clicked(move |_| {
                    kill_sender.input(MainWindowMsg::KillInstall(kill_dir.clone()));
                });
                actions.append(&kill_button);
            } else if installing {
                let resume_dir = capsule.capsule_dir.clone();
                let resume_sender = sender.clone();
                let resume_button = Button::with_label("Resume setup");
                resume_button.add_css_class("suggested-action");
                resume_button.connect_clicked(move |_| {
                    resume_sender.input(MainWindowMsg::ResumeInstall(resume_dir.clone()));
                });
                actions.append(&resume_button);
            }

            card.append(&header);
            card.append(&detail);
            card.append(&actions);
            list.append(&card);
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
            set_default_width: 1100,
            set_default_height: 720,

            #[wrap(Some)]
            set_child = &Box {
                set_orientation: Orientation::Vertical,
                set_spacing: 0,
                set_hexpand: true,
                set_vexpand: true,

                // Header bar
                append = &Box {
                    set_orientation: Orientation::Horizontal,
                    set_spacing: 12,
                    set_margin_start: 20,
                    set_margin_end: 20,
                    set_margin_top: 16,
                    set_margin_bottom: 12,
                    set_css_classes: &["topbar"],

                    append = &Image {
                        set_icon_name: Some("applications-games-symbolic"),
                        set_pixel_size: 28,
                    },

                    append = &Box {
                        set_orientation: Orientation::Vertical,
                        set_spacing: 2,

                        append = &Label {
                            set_label: "LinuxBoy",
                            set_css_classes: &["app-title"],
                            set_halign: gtk4::Align::Start,
                        },

                        append = &Label {
                            set_label: "Portable game manager for Proton-GE",
                            set_css_classes: &["muted"],
                            set_halign: gtk4::Align::Start,
                        },
                    },

                    append = &Box {
                        set_hexpand: true,
                    },

                    append = &Button {
                        set_css_classes: &["secondary"],
                        #[wrap(Some)]
                        set_child = &Box {
                            set_orientation: Orientation::Horizontal,
                            set_spacing: 6,

                            append = &Image {
                                set_icon_name: Some("preferences-system-symbolic"),
                                set_pixel_size: 16,
                            },

                            append = &Label {
                                set_label: "System Setup",
                            },
                        },
                        connect_clicked => MainWindowMsg::OpenSystemSetup,
                    },

                    append = &Button {
                        set_css_classes: &["accent"],
                        #[wrap(Some)]
                        set_child = &Box {
                            set_orientation: Orientation::Horizontal,
                            set_spacing: 6,

                            append = &Image {
                                set_icon_name: Some("list-add-symbolic"),
                                set_pixel_size: 16,
                            },

                            append = &Label {
                                set_label: "Add Game",
                            },
                        },
                        connect_clicked => MainWindowMsg::OpenInstaller,
                    },
                },

                // Tabs
                append = &Box {
                    set_orientation: Orientation::Horizontal,
                    set_margin_start: 20,
                    set_margin_end: 20,
                    set_margin_bottom: 8,

                    append = &StackSwitcher {
                        set_stack: Some(&main_stack),
                        set_css_classes: &["tab-switcher"],
                    },
                },

                // Main content area
                append = &Box {
                    set_orientation: Orientation::Vertical,
                    set_hexpand: true,
                    set_vexpand: true,
                    set_margin_start: 12,
                    set_margin_end: 12,

                    #[local_ref]
                    main_stack -> Stack {},
                },

                // Status bar
                append = &Box {
                    set_orientation: Orientation::Horizontal,
                    set_spacing: 12,
                    set_margin_start: 20,
                    set_margin_end: 20,
                    set_margin_top: 8,
                    set_margin_bottom: 16,
                    set_css_classes: &["status-bar"],

                    append = &Label {
                        #[watch]
                        set_label: &format!("{} games", model.capsules.len()),
                        set_css_classes: &["muted"],
                    },

                    append = &Box {
                        set_hexpand: true,
                    },

                    append = &Label {
                        #[watch]
                        set_label: &match model.system_check.status {
                            SystemStatus::AllInstalled => "System Ready",
                            SystemStatus::PartiallyInstalled => "Setup Incomplete",
                            SystemStatus::NothingInstalled => "Setup Required",
                        },
                        #[watch]
                        set_css_classes: &match model.system_check.status {
                            SystemStatus::AllInstalled => ["pill", "pill-installed"],
                            SystemStatus::PartiallyInstalled => ["pill", "pill-warning"],
                            SystemStatus::NothingInstalled => ["pill", "pill-missing"],
                        },
                        set_halign: gtk4::Align::End,
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

        let games_list = Box::new(Orientation::Vertical, 16);
        games_list.set_margin_all(12);
        games_list.set_valign(gtk4::Align::Start);
        games_list.set_hexpand(true);

        let library_count_label = Label::new(None);
        library_count_label.set_css_classes(&["muted"]);
        library_count_label.set_halign(gtk4::Align::Start);

        let system_status_label = Label::new(None);
        system_status_label.set_halign(gtk4::Align::Start);

        let system_status_detail = Label::new(None);
        system_status_detail.set_css_classes(&["muted"]);
        system_status_detail.set_halign(gtk4::Align::Start);
        system_status_detail.set_wrap(true);

        let main_stack = Stack::new();
        main_stack.set_hexpand(true);
        main_stack.set_vexpand(true);
        main_stack.set_transition_type(StackTransitionType::SlideLeftRight);

        let library_page = Box::new(Orientation::Vertical, 16);
        library_page.set_margin_all(20);
        library_page.set_hexpand(true);
        library_page.set_vexpand(true);

        let library_header = Box::new(Orientation::Horizontal, 12);
        library_header.set_hexpand(true);

        let library_icon = Image::from_icon_name("folder-open-symbolic");
        library_icon.set_pixel_size(24);

        let library_title = Label::new(Some("Library"));
        library_title.set_css_classes(&["section-title"]);
        library_title.set_halign(gtk4::Align::Start);

        let library_spacer = Box::new(Orientation::Horizontal, 0);
        library_spacer.set_hexpand(true);

        library_header.append(&library_icon);
        library_header.append(&library_title);
        library_header.append(&library_spacer);
        library_header.append(&library_count_label);

        let library_card = Box::new(Orientation::Vertical, 0);
        library_card.set_css_classes(&["card"]);
        library_card.set_hexpand(true);
        library_card.set_vexpand(true);

        let games_scroller = ScrolledWindow::new();
        games_scroller.set_hexpand(true);
        games_scroller.set_vexpand(true);
        games_scroller.set_child(Some(&games_list));
        library_card.append(&games_scroller);

        library_page.append(&library_header);
        library_page.append(&library_card);

        let system_page = Box::new(Orientation::Vertical, 16);
        system_page.set_margin_all(20);
        system_page.set_hexpand(true);
        system_page.set_vexpand(true);

        let system_header = Box::new(Orientation::Horizontal, 12);
        system_header.set_hexpand(true);

        let system_icon = Image::from_icon_name("preferences-system-symbolic");
        system_icon.set_pixel_size(24);

        let system_title = Label::new(Some("System"));
        system_title.set_css_classes(&["section-title"]);
        system_title.set_halign(gtk4::Align::Start);

        system_header.append(&system_icon);
        system_header.append(&system_title);

        let system_card = Box::new(Orientation::Horizontal, 16);
        system_card.set_css_classes(&["card"]);
        system_card.set_hexpand(true);

        let status_box = Box::new(Orientation::Vertical, 6);
        status_box.set_hexpand(true);
        status_box.append(&system_status_label);
        status_box.append(&system_status_detail);

        system_card.append(&status_box);
        system_card.append(&Box::new(Orientation::Horizontal, 0));

        system_page.append(&system_header);
        system_page.append(&system_card);

        main_stack.add_titled(&library_page, Some("library"), "Library");
        main_stack.add_titled(&system_page, Some("system"), "System");

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
            library_count_label,
            system_status_label,
            system_status_detail,
            root_window: root.clone(),
        };

        model.update_library_labels();
        model.update_system_labels();

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
                        self.update_library_labels();
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
                self.update_system_labels();
                
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
                self.update_system_labels();
            }
        }
    }

}
