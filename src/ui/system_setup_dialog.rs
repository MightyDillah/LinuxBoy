use gtk4::prelude::*;
use gtk4::{Dialog, Box, Label, Button, Orientation, ScrolledWindow};
use relm4::{ComponentParts, ComponentSender, RelmWidgetExt, SimpleComponent};

use crate::core::system_checker::SystemCheck;
use crate::core::runtime_manager::RuntimeManager;

#[derive(Debug)]
pub enum SystemSetupMsg {
    DownloadProton,
    Close,
}

pub struct SystemSetupDialog {
    system_check: SystemCheck,
    runtime_mgr: RuntimeManager,
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
            set_default_width: 600,
            set_default_height: 400,

            #[wrap(Some)]
            set_child = &Box {
                set_orientation: Orientation::Vertical,
                set_spacing: 10,
                set_margin_all: 20,

                // Header
                append = &Label {
                    set_markup: "<big><b>LinuxBoy System Setup</b></big>",
                    set_halign: gtk4::Align::Start,
                },

                // Status section
                append = &Box {
                    set_orientation: Orientation::Vertical,
                    set_spacing: 5,
                    set_margin_top: 10,

                    append = &Label {
                        set_markup: "<b>System Components</b>",
                        set_halign: gtk4::Align::Start,
                    },

                    append = &Label {
                        set_label: &format!("Vulkan: {}", 
                            if model.system_check.vulkan_installed { "✓ Installed" } else { "✗ Missing" }
                        ),
                        set_halign: gtk4::Align::Start,
                    },

                    append = &Label {
                        set_label: &format!("Mesa Drivers: {}", 
                            if model.system_check.mesa_installed { "✓ Installed" } else { "✗ Missing" }
                        ),
                        set_halign: gtk4::Align::Start,
                    },

                    append = &Label {
                        set_label: &format!("Proton-GE: {}", 
                            if model.system_check.proton_installed { "✓ Installed" } else { "✗ Not Downloaded" }
                        ),
                        set_halign: gtk4::Align::Start,
                    },

                    append = &Label {
                        set_label: &format!("Wine: {}", 
                            if model.system_check.wine_installed { "✓ Installed" } else { "✗ Not Installed" }
                        ),
                        set_halign: gtk4::Align::Start,
                    },
                },

                // Missing packages section
                append = &Box {
                    set_orientation: Orientation::Vertical,
                    set_spacing: 5,
                    set_margin_top: 15,
                    #[watch]
                    set_visible: !model.system_check.missing_apt_packages.is_empty(),

                    append = &Label {
                        set_markup: "<b>Missing System Packages</b>",
                        set_halign: gtk4::Align::Start,
                    },

                    append = &Label {
                        set_label: "Install these packages with:",
                        set_halign: gtk4::Align::Start,
                    },

                    append = &ScrolledWindow {
                        set_height_request: 60,
                        set_hexpand: true,

                        #[wrap(Some)]
                        set_child = &gtk4::TextView {
                            set_editable: false,
                            set_monospace: true,
                            set_wrap_mode: gtk4::WrapMode::Word,
                            #[watch]
                            set_buffer: Some(&{
                                let buffer = gtk4::TextBuffer::new(None::<&gtk4::TextTagTable>);
                                buffer.set_text(&format!("sudo apt install {}", 
                                    model.system_check.missing_apt_packages.join(" ")
                                ));
                                buffer
                            }),
                        },
                    },
                },

                // Proton-GE section
                append = &Box {
                    set_orientation: Orientation::Vertical,
                    set_spacing: 5,
                    set_margin_top: 15,

                    append = &Label {
                        set_markup: "<b>Proton-GE Runtime</b>",
                        set_halign: gtk4::Align::Start,
                    },

                    append = &Label {
                        #[watch]
                        set_label: if model.system_check.proton_installed {
                            "Proton-GE is already installed"
                        } else {
                            "Proton-GE is required to run Windows games"
                        },
                        set_halign: gtk4::Align::Start,
                    },

                    append = &Button {
                        set_label: "Download Latest Proton-GE (GE-Proton10-28)",
                        #[watch]
                        set_sensitive: !model.system_check.proton_installed,
                        connect_clicked => SystemSetupMsg::DownloadProton,
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
        };

        let widgets = view_output!();
        
        // Show the dialog
        root.present();

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            SystemSetupMsg::DownloadProton => {
                println!("Starting Proton-GE download...");
                
                match self.runtime_mgr.get_latest_release() {
                    Ok(release) => {
                        println!("Found release: {}", release.tag_name);
                        
                        match self.runtime_mgr.install_proton_ge(&release) {
                            Ok(path) => {
                                println!("✓ Proton-GE installed successfully to: {:?}", path);
                                // Refresh system check
                                self.system_check = SystemCheck::check();
                            }
                            Err(e) => {
                                eprintln!("✗ Download failed: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("✗ Failed to fetch releases: {}", e);
                    }
                }
            }
            SystemSetupMsg::Close => {
                // Dialog closes when button is clicked
                println!("Closing system setup dialog");
            }
        }
    }
}
