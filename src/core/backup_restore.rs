use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use super::capsule::Capsule;

pub struct BackupManager;

impl BackupManager {
    /// Export a capsule to a tar.gz archive
    pub fn export_capsule(capsule: &Capsule, output_path: &Path) -> Result<()> {
        println!("Exporting capsule: {}", capsule.name);

        let temp_dir = std::env::temp_dir().join(format!("linuxboy-export-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir)?;

        // Create manifest
        let manifest = ExportManifest {
            name: capsule.name.clone(),
            appimage_name: capsule
                .appimage_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("game.AppImage")
                .to_string(),
            metadata: capsule.metadata.clone(),
            version: "1.0".to_string(),
        };

        let manifest_path = temp_dir.join("manifest.json");
        let manifest_content = serde_json::to_string_pretty(&manifest)?;
        fs::write(&manifest_path, manifest_content)?;

        // Copy AppImage
        let appimage_dest = temp_dir.join(&manifest.appimage_name);
        fs::copy(&capsule.appimage_path, &appimage_dest)?;

        // Copy .home directory if exists
        if capsule.home_path.exists() {
            let home_dest = temp_dir.join(format!("{}.home", manifest.appimage_name));
            copy_dir_all(&capsule.home_path, &home_dest)?;
        }

        // Create tar.gz archive
        Self::create_archive(&temp_dir, output_path)?;

        // Cleanup temp directory
        fs::remove_dir_all(&temp_dir)?;

        println!("Export complete: {:?}", output_path);
        Ok(())
    }

    /// Import a capsule from a tar.gz archive
    pub fn import_capsule(archive_path: &Path, games_dir: &Path) -> Result<Capsule> {
        println!("Importing capsule from: {:?}", archive_path);

        let temp_dir = std::env::temp_dir().join(format!("linuxboy-import-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir)?;

        // Extract archive
        Self::extract_archive(archive_path, &temp_dir)?;

        // Read manifest
        let manifest_path = temp_dir.join("manifest.json");
        let manifest_content = fs::read_to_string(&manifest_path)
            .context("Missing manifest.json in archive")?;
        let manifest: ExportManifest = serde_json::from_str(&manifest_content)?;

        // Move AppImage to games directory
        fs::create_dir_all(games_dir)?;
        let appimage_src = temp_dir.join(&manifest.appimage_name);
        let appimage_dest = games_dir.join(&manifest.appimage_name);

        if appimage_dest.exists() {
            anyhow::bail!("Game already exists: {:?}", appimage_dest);
        }

        fs::copy(&appimage_src, &appimage_dest)?;

        // Make executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&appimage_dest)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&appimage_dest, perms)?;
        }

        // Move .home directory
        let home_src = temp_dir.join(format!("{}.home", manifest.appimage_name));
        let home_dest = games_dir.join(format!("{}.home", manifest.appimage_name));

        if home_src.exists() {
            copy_dir_all(&home_src, &home_dest)?;
        }

        // Cleanup
        fs::remove_dir_all(&temp_dir)?;

        // Load and return the capsule
        let capsule = Capsule::load_from_appimage(&appimage_dest)?;

        println!("Import complete: {}", capsule.name);
        Ok(capsule)
    }

    fn create_archive(source_dir: &Path, output_path: &Path) -> Result<()> {
        let tar_gz = File::create(output_path)?;
        let enc = flate2::write::GzEncoder::new(tar_gz, flate2::Compression::default());
        let mut tar = tar::Builder::new(enc);

        tar.append_dir_all(".", source_dir)?;
        tar.finish()?;

        Ok(())
    }

    fn extract_archive(archive_path: &Path, dest_dir: &Path) -> Result<()> {
        let tar_gz = File::open(archive_path)?;
        let dec = flate2::read::GzDecoder::new(tar_gz);
        let mut archive = tar::Archive::new(dec);

        archive.unpack(dest_dir)?;

        Ok(())
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ExportManifest {
    name: String,
    appimage_name: String,
    metadata: crate::core::capsule::CapsuleMetadata,
    version: String,
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
