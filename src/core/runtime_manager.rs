use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::fs::{self, File, OpenOptions};
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

#[derive(Clone)]
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

    /// Download a file from URL with resume support and progress callback
    pub fn download_file<F>(
        &self,
        url: &str,
        dest_path: &Path,
        expected_size: Option<u64>,
        mut progress_callback: F,
    ) -> Result<()>
    where
        F: FnMut(u64, u64),  // (downloaded_bytes, total_bytes)
    {
        println!("Downloading: {}", url);
        println!("Destination: {:?}", dest_path);

        let client = reqwest::blocking::Client::builder()
            .user_agent("LinuxBoy/0.1")
            .build()?;

        let expected_size = expected_size.filter(|size| *size > 0);
        let filename = dest_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("download");
        let temp_path = dest_path.with_file_name(format!("{}.part", filename));

        // Create parent directory if needed
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // If we already have a full file, reuse it
        if dest_path.exists() {
            if let Some(expected) = expected_size {
                let existing = dest_path.metadata()?.len();
                if existing == expected {
                    progress_callback(existing, expected);
                    return Ok(());
                }

                if existing < expected {
                    let _ = fs::rename(dest_path, &temp_path);
                } else {
                    fs::remove_file(dest_path)?;
                }
            } else {
                return Ok(());
            }
        }

        let mut existing = if temp_path.exists() {
            temp_path.metadata()?.len()
        } else {
            0
        };

        if let Some(expected) = expected_size {
            if existing > expected {
                fs::remove_file(&temp_path)?;
                existing = 0;
            }
        }

        let mut response = if existing > 0 {
            client
                .get(url)
                .header(reqwest::header::RANGE, format!("bytes={}-", existing))
                .send()?
        } else {
            client.get(url).send()?
        };

        if !response.status().is_success() {
            anyhow::bail!("Download failed with status: {}", response.status());
        }

        if existing > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            // Range requests are not supported; restart from scratch.
            existing = 0;
            if temp_path.exists() {
                fs::remove_file(&temp_path)?;
            }
            response = client.get(url).send()?;
            if !response.status().is_success() {
                anyhow::bail!("Download failed with status: {}", response.status());
            }
        }

        let segment_size = response.content_length().unwrap_or(0);
        let total_size = expected_size.unwrap_or_else(|| {
            if segment_size > 0 {
                existing + segment_size
            } else {
                0
            }
        });

        let mut file = if existing > 0 {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&temp_path)?
        } else {
            File::create(&temp_path)?
        };

        let mut downloaded: u64 = existing;
        progress_callback(downloaded, total_size);

        let mut buffer = [0u8; 8192];
        loop {
            let bytes_read = response.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }

            file.write_all(&buffer[..bytes_read])?;
            downloaded += bytes_read as u64;

            // Report progress
            progress_callback(downloaded, total_size);
        }

        if let Some(expected) = expected_size {
            if downloaded < expected {
                anyhow::bail!("Download incomplete: {} / {} bytes", downloaded, expected);
            }
            if downloaded > expected {
                let _ = fs::remove_file(&temp_path);
                anyhow::bail!("Download size mismatch: {} / {} bytes", downloaded, expected);
            }
        }

        fs::rename(&temp_path, dest_path)?;
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

    /// Download and install Proton-GE with progress callback
    pub fn install_proton_ge<F>(
        &self,
        release: &ProtonRelease,
        reinstall: bool,
        mut progress_callback: F,
    ) -> Result<PathBuf>
    where
        F: FnMut(String, f64),  // (status_text, progress_fraction)
    {
        // Find the tar.gz asset
        let targz_asset = Self::find_targz_asset(release)
            .context("No .tar.gz file found in release")?;

        let filename = &targz_asset.name;
        let download_url = &targz_asset.browser_download_url;

        // Create downloads cache directory
        let cache_dir = self.runtimes_dir.parent().unwrap().join("cache/downloads");
        fs::create_dir_all(&cache_dir)?;

        let download_path = cache_dir.join(filename);

        let partial_path = cache_dir.join(format!("{}.part", filename));
        let expected_size = if targz_asset.size > 0 {
            Some(targz_asset.size)
        } else {
            None
        };

        if reinstall {
            let _ = fs::remove_file(&download_path);
            let _ = fs::remove_file(&partial_path);
        }

        // Download if not already cached (or if size doesn't match)
        let mut cached_size = 0;
        if download_path.exists() {
            cached_size = download_path.metadata()?.len();
        }

        if !download_path.exists() || expected_size.map(|size| cached_size != size).unwrap_or(false) {
            let total_mb = targz_asset.size / 1_048_576;
            println!("Downloading {} ({} MB)...", filename, total_mb);

            let mut resume_bytes = 0;
            if partial_path.exists() {
                resume_bytes = partial_path.metadata()?.len();
            } else if download_path.exists() && expected_size.map(|size| cached_size < size).unwrap_or(false) {
                resume_bytes = cached_size;
            }

            if resume_bytes > 0 && targz_asset.size > 0 {
                let resume_mb = resume_bytes / 1_048_576;
                progress_callback(
                    format!("Resuming {} ({} / {} MB)", filename, resume_mb, total_mb),
                    (resume_bytes as f64 / targz_asset.size as f64) * 0.9,
                );
            } else {
                progress_callback(format!("Downloading {} (0 / {} MB)", filename, total_mb), 0.0);
            }

            self.download_file(download_url, &download_path, expected_size, |downloaded, total| {
                if total > 0 {
                    let progress = downloaded as f64 / total as f64;
                    let downloaded_mb = downloaded / 1_048_576;
                    let total_mb = total / 1_048_576;
                    progress_callback(
                        format!("Downloading {} ({} / {} MB)", filename, downloaded_mb, total_mb),
                        progress * 0.9,  // Reserve 10% for extraction
                    );
                }
            })?;
        } else {
            println!("Using cached file: {:?}", download_path);
            progress_callback(format!("Using cached file: {}", filename), 0.9);
        }

        // Extract to staging directory
        fs::create_dir_all(&self.runtimes_dir)?;
        let staging_dir = self
            .runtimes_dir
            .join(format!(".staging-{}", release.tag_name));
        if staging_dir.exists() {
            fs::remove_dir_all(&staging_dir)?;
        }

        println!("Extracting to {:?}...", staging_dir);
        progress_callback("Extracting archive...".to_string(), 0.95);

        if let Err(e) = self.extract_targz(&download_path, &staging_dir) {
            let _ = fs::remove_dir_all(&staging_dir);
            return Err(e);
        }

        let preferred_dir = staging_dir.join(&release.tag_name);
        let extracted_dir = if preferred_dir.exists() {
            preferred_dir
        } else {
            let mut found_dir = None;
            for entry in fs::read_dir(&staging_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    found_dir = Some(entry.path());
                    break;
                }
            }
            found_dir.unwrap_or_else(|| staging_dir.clone())
        };

        let extracted_name = if extracted_dir == staging_dir {
            release.tag_name.clone()
        } else {
            extracted_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&release.tag_name)
                .to_string()
        };
        let final_dir = self.runtimes_dir.join(&extracted_name);

        if final_dir.exists() {
            if reinstall {
                fs::remove_dir_all(&final_dir)?;
            } else {
                let _ = fs::remove_dir_all(&staging_dir);
                progress_callback("Proton-GE already installed.".to_string(), 1.0);
                return Ok(final_dir);
            }
        }

        fs::rename(&extracted_dir, &final_dir)?;
        if staging_dir.exists() {
            let _ = fs::remove_dir_all(&staging_dir);
        }

        println!("Proton-GE installed successfully!");
        progress_callback("Installation complete!".to_string(), 1.0);

        Ok(final_dir)
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

    /// Get the path to the latest installed Proton-GE version
    pub fn latest_installed(&self) -> Result<Option<PathBuf>> {
        let mut installed = self.list_installed()?;
        if installed.is_empty() {
            return Ok(None);
        }

        installed.sort();
        let latest = installed.last().cloned().unwrap_or_default();
        Ok(self.get_proton_path(&latest))
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
