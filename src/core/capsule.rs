use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capsule {
    pub name: String,
    pub appimage_path: PathBuf,
    pub home_path: PathBuf,
    pub metadata: CapsuleMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapsuleMetadata {
    pub name: String,
    pub executables: ExecutableConfig,
    pub wine_version: Option<String>,
    pub dxvk_enabled: bool,
    pub vkd3d_enabled: bool,
    pub env_vars: Vec<(String, String)>,
    pub redistributables_installed: Vec<String>,
    #[serde(default)]
    pub last_played: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableConfig {
    pub main: ExecutableEntry,
    #[serde(default)]
    pub tools: Vec<ExecutableEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutableEntry {
    pub path: String,
    pub args: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_shortcut: Option<String>,
}

impl Capsule {
    /// Scan a directory for .AppImage files and load their metadata
    pub fn scan_directory(dir: &Path) -> Result<Vec<Capsule>> {
        let mut capsules = Vec::new();
        
        if !dir.exists() {
            fs::create_dir_all(dir)?;
            return Ok(capsules);
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.extension().and_then(|s| s.to_str()) == Some("AppImage") {
                if let Ok(capsule) = Self::load_from_appimage(&path) {
                    capsules.push(capsule);
                }
            }
        }

        Ok(capsules)
    }

    /// Load capsule information from an AppImage file
    pub fn load_from_appimage(appimage_path: &Path) -> Result<Capsule> {
        let home_path = Self::get_home_path(appimage_path);
        let metadata_path = home_path.join("metadata.json");

        let metadata = if metadata_path.exists() {
            let content = fs::read_to_string(&metadata_path)
                .context("Failed to read metadata.json")?;
            serde_json::from_str(&content)
                .context("Failed to parse metadata.json")?
        } else {
            // Create default metadata if none exists
            let name = appimage_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string();
            
            CapsuleMetadata {
                name: name.clone(),
                executables: ExecutableConfig {
                    main: ExecutableEntry {
                        path: String::new(),
                        args: String::new(),
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
        };

        Ok(Capsule {
            name: metadata.name.clone(),
            appimage_path: appimage_path.to_path_buf(),
            home_path,
            metadata,
        })
    }

    /// Get the .home directory path for an AppImage
    pub fn get_home_path(appimage_path: &Path) -> PathBuf {
        let mut home = appimage_path.to_path_buf();
        let name = home.file_name().unwrap().to_str().unwrap();
        home.set_file_name(format!("{}.home", name));
        home
    }

    /// Save metadata to disk
    pub fn save_metadata(&self) -> Result<()> {
        if !self.home_path.exists() {
            fs::create_dir_all(&self.home_path)?;
        }

        let metadata_path = self.home_path.join("metadata.json");
        let content = serde_json::to_string_pretty(&self.metadata)?;
        fs::write(&metadata_path, content)?;

        Ok(())
    }

    /// Launch the capsule
    pub fn launch(&self) -> Result<()> {
        if !self.appimage_path.exists() {
            anyhow::bail!("AppImage not found: {:?}", self.appimage_path);
        }

        // TODO: Implement launch logic
        println!("Launching: {}", self.name);
        Ok(())
    }

    /// Remove the capsule (delete AppImage and .home directory)
    pub fn remove(&self) -> Result<()> {
        if self.appimage_path.exists() {
            fs::remove_file(&self.appimage_path)?;
        }

        if self.home_path.exists() {
            fs::remove_dir_all(&self.home_path)?;
        }

        Ok(())
    }
}

impl Default for CapsuleMetadata {
    fn default() -> Self {
        Self {
            name: String::new(),
            executables: ExecutableConfig {
                main: ExecutableEntry {
                    path: String::new(),
                    args: String::new(),
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
}
