use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::capsule::{CapsuleMetadata, ExecutableConfig, ExecutableEntry};

pub struct AppImageBuilder {
    pub source_path: PathBuf,
    pub output_dir: PathBuf,
    pub game_name: String,
}

impl AppImageBuilder {
    pub fn new(source_path: PathBuf, output_dir: PathBuf, game_name: String) -> Self {
        Self {
            source_path,
            output_dir,
            game_name,
        }
    }

    /// Build AppImage from a portable game folder
    pub fn build_from_folder(&self, main_exe: &str, launch_args: &str) -> Result<PathBuf> {
        println!("Building AppImage from folder: {:?}", self.source_path);

        let appdir = self.create_appdir()?;
        
        // Copy game files
        self.copy_game_files(&appdir)?;
        
        // Create AppRun script
        self.create_apprun_script(&appdir, main_exe, launch_args)?;
        
        // Create metadata
        let metadata = self.create_metadata(main_exe, launch_args);
        self.save_metadata_template(&appdir, &metadata)?;
        
        // Package into AppImage
        let appimage_path = self.package_appimage(&appdir)?;
        
        // Cleanup temporary AppDir
        fs::remove_dir_all(&appdir)?;
        
        Ok(appimage_path)
    }

    /// Build AppImage from an installer (.exe, .msi)
    pub fn build_from_installer(
        &self,
        installer_path: &Path,
        wine_path: &str,
        selected_exe: &str,
        launch_args: &str,
    ) -> Result<PathBuf> {
        println!("Building AppImage from installer: {:?}", installer_path);

        // Create temporary Wine prefix for installation
        let temp_prefix = self.create_temp_prefix()?;
        
        // Run installer
        self.run_installer(installer_path, wine_path, &temp_prefix)?;
        
        // Detect shortcuts
        let shortcuts = self.detect_shortcuts(&temp_prefix)?;
        
        // Create AppDir
        let appdir = self.create_appdir()?;
        
        // Copy installed game files
        self.copy_installed_game(&temp_prefix, &appdir)?;
        
        // Create AppRun script
        self.create_apprun_script(&appdir, selected_exe, launch_args)?;
        
        // Create metadata
        let metadata = self.create_metadata_with_shortcuts(selected_exe, launch_args, shortcuts);
        self.save_metadata_template(&appdir, &metadata)?;
        
        // Package into AppImage
        let appimage_path = self.package_appimage(&appdir)?;
        
        // Cleanup
        fs::remove_dir_all(&appdir)?;
        fs::remove_dir_all(&temp_prefix)?;
        
        Ok(appimage_path)
    }

    fn create_appdir(&self) -> Result<PathBuf> {
        let appdir = self.output_dir.join(format!("{}.AppDir", self.game_name));
        fs::create_dir_all(&appdir)?;
        Ok(appdir)
    }

    fn copy_game_files(&self, appdir: &Path) -> Result<()> {
        let game_dir = appdir.join("game");
        fs::create_dir_all(&game_dir)?;
        
        // Copy all files from source to game directory
        copy_dir_all(&self.source_path, &game_dir)?;
        
        Ok(())
    }

    fn copy_installed_game(&self, prefix: &Path, appdir: &Path) -> Result<()> {
        let game_dir = appdir.join("game");
        fs::create_dir_all(&game_dir)?;
        
        // Find Program Files and copy game installation
        let program_files = prefix.join("drive_c/Program Files");
        let program_files_x86 = prefix.join("drive_c/Program Files (x86)");
        
        // TODO: Let user select which folder to package
        // For now, copy first found directory
        if program_files.exists() {
            for entry in fs::read_dir(program_files)? {
                let entry = entry?;
                if entry.path().is_dir() {
                    copy_dir_all(&entry.path(), &game_dir.join(entry.file_name()))?;
                    break; // Copy first directory for now
                }
            }
        }
        
        Ok(())
    }

    fn create_apprun_script(&self, appdir: &Path, main_exe: &str, launch_args: &str) -> Result<()> {
        let apprun_path = appdir.join("AppRun");
        
        let script = include_str!("../../capsule-runtime/AppRun.sh")
            .replace("{{GAME_EXE}}", main_exe)
            .replace("{{LAUNCH_ARGS}}", launch_args);
        
        fs::write(&apprun_path, script)?;
        
        // Make executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&apprun_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&apprun_path, perms)?;
        }
        
        Ok(())
    }

    fn create_metadata(&self, main_exe: &str, launch_args: &str) -> CapsuleMetadata {
        CapsuleMetadata {
            name: self.game_name.clone(),
            executables: ExecutableConfig {
                main: ExecutableEntry {
                    path: format!("game/{}", main_exe),
                    args: launch_args.to_string(),
                    label: "Launch".to_string(),
                    original_shortcut: None,
                },
                tools: Vec::new(),
            },
            wine_version: None,
            dxvk_enabled: true,
            vkd3d_enabled: false,
            env_vars: Vec::new(),
            redistributables_installed: Vec::new(),
            last_played: None,
        }
    }

    fn create_metadata_with_shortcuts(
        &self,
        main_exe: &str,
        launch_args: &str,
        shortcuts: Vec<String>,
    ) -> CapsuleMetadata {
        let mut metadata = self.create_metadata(main_exe, launch_args);
        // TODO: Parse shortcuts and add as tools
        metadata
    }

    fn save_metadata_template(&self, appdir: &Path, metadata: &CapsuleMetadata) -> Result<()> {
        let metadata_path = appdir.join("metadata.template.json");
        let content = serde_json::to_string_pretty(metadata)?;
        fs::write(metadata_path, content)?;
        Ok(())
    }

    fn package_appimage(&self, appdir: &Path) -> Result<PathBuf> {
        let appimage_path = self.output_dir.join(format!("{}.AppImage", self.game_name));
        
        // Use appimagetool to create AppImage
        let status = Command::new("appimagetool")
            .arg(appdir)
            .arg(&appimage_path)
            .status()
            .context("Failed to run appimagetool. Is it installed?")?;
        
        if !status.success() {
            anyhow::bail!("appimagetool failed to create AppImage");
        }
        
        Ok(appimage_path)
    }

    fn create_temp_prefix(&self) -> Result<PathBuf> {
        let temp_dir = std::env::temp_dir();
        let prefix = temp_dir.join(format!("linuxboy-install-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&prefix)?;
        Ok(prefix)
    }

    fn run_installer(&self, installer_path: &Path, wine_path: &str, prefix: &Path) -> Result<()> {
        let status = Command::new(wine_path)
            .arg(installer_path)
            .env("WINEPREFIX", prefix)
            .status()
            .context("Failed to run Wine installer")?;
        
        if !status.success() {
            anyhow::bail!("Installer failed or was cancelled");
        }
        
        Ok(())
    }

    fn detect_shortcuts(&self, prefix: &Path) -> Result<Vec<String>> {
        let mut shortcuts = Vec::new();
        
        let start_menu = prefix.join("drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs");
        
        if start_menu.exists() {
            for entry in walkdir::WalkDir::new(start_menu)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.path().extension().and_then(|s| s.to_str()) == Some("lnk") {
                    if let Some(name) = entry.path().file_name().and_then(|s| s.to_str()) {
                        // Skip common unwanted shortcuts
                        if !name.to_lowercase().contains("uninstall")
                            && !name.to_lowercase().contains("readme")
                            && !name.to_lowercase().contains("website")
                        {
                            shortcuts.push(name.to_string());
                        }
                    }
                }
            }
        }
        
        Ok(shortcuts)
    }
}

/// Recursively copy directory contents
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            fs::copy(entry.path(), dst_path)?;
        }
    }
    
    Ok(())
}
