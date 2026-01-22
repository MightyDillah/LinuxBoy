use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const UMU_DATABASE_URL: &str = "https://umu.openwinecomponents.org/umu_api.php";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UmuEntry {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub umu_id: Option<String>,
    #[serde(default)]
    pub acronym: Option<String>,
    #[serde(default)]
    pub codename: Option<String>,
    #[serde(default)]
    pub store: Option<String>,
    #[serde(default)]
    pub exe_string: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

pub struct UmuDatabase;

impl UmuDatabase {
    pub fn load_or_fetch() -> Result<Vec<UmuEntry>> {
        match Self::fetch_entries() {
            Ok(entries) => {
                let _ = Self::write_cache(&entries);
                Ok(entries)
            }
            Err(fetch_err) => {
                if let Ok(entries) = Self::read_cache() {
                    Ok(entries)
                } else {
                    Err(fetch_err)
                }
            }
        }
    }

    pub fn normalize_title(title: &str) -> String {
        title
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .flat_map(|ch| ch.to_lowercase())
            .collect()
    }

    fn fetch_entries() -> Result<Vec<UmuEntry>> {
        let response = reqwest::blocking::get(UMU_DATABASE_URL)
            .context("Failed to request UMU database")?;
        response
            .json::<Vec<UmuEntry>>()
            .context("Failed to parse UMU database response")
    }

    fn read_cache() -> Result<Vec<UmuEntry>> {
        let path = Self::cache_path().context("Home directory not available")?;
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read UMU cache at {:?}", path))?;
        serde_json::from_str(&content).context("Failed to parse UMU cache file")
    }

    fn write_cache(entries: &[UmuEntry]) -> Result<()> {
        let path = Self::cache_path().context("Home directory not available")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create UMU cache dir {:?}", parent))?;
        }
        let content = serde_json::to_string(entries).context("Failed to serialize UMU cache")?;
        fs::write(&path, content)
            .with_context(|| format!("Failed to write UMU cache at {:?}", path))?;
        Ok(())
    }

    fn cache_path() -> Option<PathBuf> {
        dirs::home_dir().map(|home| home.join(".linuxboy").join("cache").join("umu_database.json"))
    }
}
