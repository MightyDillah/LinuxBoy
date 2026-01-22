use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum InstallState {
    Installing,
    Installed,
}

impl Default for InstallState {
    fn default() -> Self {
        Self::Installing
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capsule {
    pub name: String,
    pub capsule_dir: PathBuf,
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
    #[serde(default)]
    pub installer_path: Option<String>,
    #[serde(default)]
    pub install_state: InstallState,
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
    /// Scan a directory for capsule folders with metadata.json
    pub fn scan_directory(dir: &Path) -> Result<Vec<Capsule>> {
        let mut capsules = Vec::new();
        
        if !dir.exists() {
            fs::create_dir_all(dir)?;
            return Ok(capsules);
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_dir() {
                if let Ok(capsule) = Self::load_from_dir(&path) {
                    capsules.push(capsule);
                }
            }
        }

        Ok(capsules)
    }

    /// Load capsule information from a capsule directory
    pub fn load_from_dir(capsule_dir: &Path) -> Result<Capsule> {
        let metadata_path = capsule_dir.join("metadata.json");
        let content = fs::read_to_string(&metadata_path)
            .context("Failed to read metadata.json")?;
        let metadata: CapsuleMetadata = serde_json::from_str(&content)
            .context("Failed to parse metadata.json")?;

        let name = metadata.name.clone();
        let home_path = capsule_dir.join(format!("{}.AppImage.home", name));

        Ok(Capsule {
            name,
            capsule_dir: capsule_dir.to_path_buf(),
            home_path,
            metadata,
        })
    }

    pub fn save_metadata(&self) -> Result<()> {
        let metadata_path = self.capsule_dir.join("metadata.json");
        let content = serde_json::to_string_pretty(&self.metadata)
            .context("Failed to serialize metadata.json")?;
        fs::write(&metadata_path, content)
            .context("Failed to write metadata.json")?;
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
            installer_path: None,
            install_state: InstallState::Installing,
        }
    }
}
