use anyhow::Result;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub gpu_info: String,
    pub vulkan_available: bool,
    pub wine_available: bool,
    pub wine_version: Option<String>,
    pub kernel_version: String,
    pub glibc_version: String,
}

impl SystemInfo {
    pub fn detect() -> Result<Self> {
        Ok(Self {
            gpu_info: Self::detect_gpu(),
            vulkan_available: Self::check_vulkan(),
            wine_available: Self::check_wine().is_some(),
            wine_version: Self::check_wine(),
            kernel_version: Self::detect_kernel(),
            glibc_version: Self::detect_glibc(),
        })
    }

    fn detect_gpu() -> String {
        let output = Command::new("lspci")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();

        for line in output.lines() {
            if line.contains("VGA") || line.contains("3D") {
                return line.to_string();
            }
        }

        "Unknown GPU".to_string()
    }

    fn check_vulkan() -> bool {
        Command::new("vulkaninfo")
            .arg("--summary")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn check_wine() -> Option<String> {
        let output = Command::new("wine64")
            .arg("--version")
            .output()
            .ok()?;

        if output.status.success() {
            String::from_utf8(output.stdout)
                .ok()
                .map(|s| s.trim().to_string())
        } else {
            None
        }
    }

    fn detect_kernel() -> String {
        let output = Command::new("uname")
            .arg("-r")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();

        output.trim().to_string()
    }

    fn detect_glibc() -> String {
        let output = Command::new("ldd")
            .arg("--version")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default();

        output
            .lines()
            .next()
            .unwrap_or("Unknown")
            .to_string()
    }
}
