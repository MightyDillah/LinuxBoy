use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const GITHUB_API_RELEASES: &str = "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtonRelease {
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

pub struct RuntimeManager {
    runtimes_dir: PathBuf,
}

#[allow(dead_code)]
impl RuntimeManager {
    pub fn new() -> Self {
        let runtimes_dir = crate::core::system_checker::SystemCheck::get_runtimes_dir();
        Self { runtimes_dir }
    }

    /// Get list of available Proton-GE releases from GitHub
    pub fn fetch_available_releases(&self) -> Result<Vec<ProtonRelease>> {
        println!("Fetching Proton-GE releases from GitHub...");
        
        let client = reqwest::blocking::Client::builder()
            .user_agent("LinuxBoy/0.1")
            .build()?;

        let response = client
            .get(GITHUB_API_RELEASES)
            .send()
            .context("Failed to fetch releases from GitHub")?;

        if !response.status().is_success() {
            anyhow::bail!("GitHub API returned status: {}", response.status());
        }

        let releases: Vec<ProtonRelease> = response
            .json()
            .context("Failed to parse GitHub releases JSON")?;

        println!("Found {} Proton-GE releases", releases.len());
        Ok(releases)
    }

    /// Get the latest Proton-GE release
    pub fn get_latest_release(&self) -> Result<ProtonRelease> {
        let releases = self.fetch_available_releases()?;
        releases.into_iter().next()
            .context("No releases found")
    }

    /// Find the tar.gz asset for a release
    pub fn find_targz_asset(release: &ProtonRelease) -> Option<&GitHubAsset> {
        release.assets.iter()
            .find(|asset| asset.name.ends_with(".tar.gz"))
    }

    /// Find the sha512sum file for a release
    pub fn find_checksum_asset(release: &ProtonRelease) -> Option<&GitHubAsset> {
        release.assets.iter()
            .find(|asset| asset.name.ends_with(".sha512sum"))
    }

    /// Download a file from URL with progress
    pub fn download_file(&self, url: &str, dest_path: &Path) -> Result<()> {
        println!("Downloading: {}", url);
        println!("Destination: {:?}", dest_path);

        let client = reqwest::blocking::Client::builder()
            .user_agent("LinuxBoy/0.1")
            .build()?;

        let response = client.get(url).send()?;
        
        if !response.status().is_success() {
            anyhow::bail!("Download failed with status: {}", response.status());
        }

        let total_size = response.content_length().unwrap_or(0);
        println!("File size: {} MB", total_size / 1_048_576);

        // Create parent directory if needed
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = File::create(dest_path)?;
        let mut buffer = Vec::new();
        
        response.bytes()?.as_ref().read_to_end(&mut buffer)?;
        file.write_all(&buffer)?;
        
        println!("Download complete!");
        Ok(())
    }

    /// Calculate SHA256 hash of a file
    pub fn calculate_sha256(&self, file_path: &Path) -> Result<String> {
        let mut file = File::open(file_path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let hash = hasher.finalize();
        Ok(hex::encode(hash))
    }

    /// Verify file checksum against expected SHA256
    pub fn verify_checksum(&self, file_path: &Path, expected_sha256: &str) -> Result<bool> {
        let actual = self.calculate_sha256(file_path)?;
        Ok(actual.to_lowercase() == expected_sha256.to_lowercase())
    }

    /// Download and install Proton-GE
    pub fn install_proton_ge(&self, release: &ProtonRelease) -> Result<PathBuf> {
        // Find the tar.gz asset
        let targz_asset = Self::find_targz_asset(release)
            .context("No .tar.gz file found in release")?;

        let filename = &targz_asset.name;
        let download_url = &targz_asset.browser_download_url;

        // Create downloads cache directory
        let cache_dir = self.runtimes_dir.parent().unwrap().join("cache/downloads");
        fs::create_dir_all(&cache_dir)?;

        let download_path = cache_dir.join(filename);

        // Download if not already cached
        if !download_path.exists() {
            println!("Downloading {} ({} MB)...", filename, targz_asset.size / 1_048_576);
            self.download_file(download_url, &download_path)?;
        } else {
            println!("Using cached file: {:?}", download_path);
        }

        // TODO: Verify checksum (need to parse .sha512sum file)
        println!("Checksum verification: Skipped (TODO)");

        // Extract to runtimes directory
        fs::create_dir_all(&self.runtimes_dir)?;
        println!("Extracting to {:?}...", self.runtimes_dir);
        self.extract_targz(&download_path, &self.runtimes_dir)?;

        // Determine extracted directory name (usually same as tag_name)
        let extracted_dir = self.runtimes_dir.join(&release.tag_name);
        
        println!("Proton-GE installed successfully!");
        Ok(extracted_dir)
    }

    /// Extract a .tar.gz file
    fn extract_targz(&self, archive_path: &Path, dest_dir: &Path) -> Result<()> {
        use flate2::read::GzDecoder;
        use tar::Archive;

        let file = File::open(archive_path)?;
        let decompressor = GzDecoder::new(file);
        let mut archive = Archive::new(decompressor);

        archive.unpack(dest_dir)
            .context("Failed to extract archive")?;

        Ok(())
    }

    /// List installed Proton-GE versions
    pub fn list_installed(&self) -> Result<Vec<String>> {
        let mut installed = Vec::new();

        if !self.runtimes_dir.exists() {
            return Ok(installed);
        }

        for entry in fs::read_dir(&self.runtimes_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("GE-Proton") {
                    installed.push(name);
                }
            }
        }

        Ok(installed)
    }

    /// Check if a specific Proton-GE version is installed
    pub fn is_installed(&self, version: &str) -> bool {
        self.runtimes_dir.join(version).exists()
    }

    /// Get path to installed Proton-GE
    pub fn get_proton_path(&self, version: &str) -> Option<PathBuf> {
        let path = self.runtimes_dir.join(version);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }
}
