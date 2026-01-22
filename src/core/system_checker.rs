use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum SystemStatus {
    AllInstalled,     // Everything ready (green)
    PartiallyInstalled, // Some components missing (orange)
    NothingInstalled,   // Critical components missing (red)
}

#[derive(Debug, Clone)]
pub struct SystemCheck {
    pub status: SystemStatus,
    pub vulkan_installed: bool,
    pub mesa_installed: bool,
    pub proton_installed: bool,
    pub missing_apt_packages: Vec<String>,
}

impl SystemCheck {
    /// Quick system check - runs on startup
    pub fn check() -> Self {
        let vulkan_installed = Self::check_command("vulkaninfo");
        let mesa_installed = Self::check_mesa();
        let proton_installed = Self::check_proton_ge();

        let mut missing_apt_packages = Vec::new();
        
        if !vulkan_installed {
            missing_apt_packages.push("vulkan-tools".to_string());
            missing_apt_packages.push("libvulkan1".to_string());
            missing_apt_packages.push("libvulkan1:i386".to_string());
        }
        
        if !mesa_installed {
            missing_apt_packages.push("mesa-vulkan-drivers".to_string());
            missing_apt_packages.push("mesa-vulkan-drivers:i386".to_string());
            missing_apt_packages.push("libgl1-mesa-dri:amd64".to_string());
            missing_apt_packages.push("libgl1-mesa-dri:i386".to_string());
            missing_apt_packages.push("libgl1-mesa-glx:amd64".to_string());
            missing_apt_packages.push("libgl1-mesa-glx:i386".to_string());
        }

        // Determine overall status
        let apt_ok = vulkan_installed && mesa_installed;
        let runtimes_ok = proton_installed;

        let status = if apt_ok && runtimes_ok {
            SystemStatus::AllInstalled
        } else if apt_ok || runtimes_ok {
            SystemStatus::PartiallyInstalled
        } else {
            SystemStatus::NothingInstalled
        };

        Self {
            status,
            vulkan_installed,
            mesa_installed,
            proton_installed,
            missing_apt_packages,
        }
    }

    /// Get the LinuxBoy config directory
    pub fn get_linuxboy_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".linuxboy")
    }

    /// Get runtimes directory
    pub fn get_runtimes_dir() -> PathBuf {
        Self::get_linuxboy_dir().join("runtimes")
    }

    /// Check if a command exists in PATH
    fn check_command(cmd: &str) -> bool {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Check if Mesa drivers are installed
    fn check_mesa() -> bool {
        // Check if mesa is installed by looking for vulkaninfo output
        if let Ok(output) = Command::new("vulkaninfo").arg("--summary").output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Look for Intel/AMD drivers (Mesa)
                return stdout.contains("Intel") || stdout.contains("AMD") || stdout.contains("RADV");
            }
        }
        false
    }

    /// Check if Proton-GE is installed in ~/.linuxboy/runtimes/
    fn check_proton_ge() -> bool {
        let runtimes_dir = Self::get_runtimes_dir();
        if !runtimes_dir.exists() {
            return false;
        }

        // Look for any proton-ge-* directory
        if let Ok(entries) = std::fs::read_dir(&runtimes_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy().to_lowercase();
                if name.starts_with("proton-ge-") || name.starts_with("ge-proton") {
                    return true;
                }
            }
        }
        false
    }

    /// Get a human-readable status message
    pub fn status_message(&self) -> String {
        match self.status {
            SystemStatus::AllInstalled => "System Ready".to_string(),
            SystemStatus::PartiallyInstalled => "Setup Incomplete".to_string(),
            SystemStatus::NothingInstalled => "Setup Required".to_string(),
        }
    }

}
