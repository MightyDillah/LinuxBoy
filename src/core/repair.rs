use anyhow::Result;
use std::fs;
use std::path::Path;

use super::capsule::Capsule;

pub struct RepairTools;

impl RepairTools {
    /// Rebuild the Wine prefix (delete and let it recreate on next launch)
    pub fn rebuild_prefix(capsule: &Capsule) -> Result<()> {
        let prefix_path = capsule.home_path.join("prefix");

        if prefix_path.exists() {
            println!("Removing old prefix: {:?}", prefix_path);
            fs::remove_dir_all(&prefix_path)?;
            println!("Prefix will be recreated on next launch");
        } else {
            println!("No prefix found to rebuild");
        }

        Ok(())
    }

    /// Clear all cache directories
    pub fn clear_cache(capsule: &Capsule) -> Result<()> {
        let cache_path = capsule.home_path.join("cache");

        if cache_path.exists() {
            println!("Clearing cache: {:?}", cache_path);
            fs::remove_dir_all(&cache_path)?;
            fs::create_dir_all(&cache_path)?;
            println!("Cache cleared");
        } else {
            println!("No cache found to clear");
        }

        Ok(())
    }

    /// Reset runtime configuration to defaults
    pub fn reset_runtime(capsule: &mut Capsule) -> Result<()> {
        println!("Resetting runtime configuration to defaults");

        capsule.metadata.wine_version = None;
        capsule.metadata.dxvk_enabled = true;
        capsule.metadata.vkd3d_enabled = false;
        capsule.metadata.env_vars.clear();

        capsule.save_metadata()?;

        println!("Runtime configuration reset");
        Ok(())
    }

    /// Verify AppImage integrity
    pub fn verify_appimage(capsule: &Capsule) -> Result<bool> {
        if !capsule.appimage_path.exists() {
            println!("AppImage not found: {:?}", capsule.appimage_path);
            return Ok(false);
        }

        // Check if file is executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&capsule.appimage_path)?;
            let permissions = metadata.permissions();

            if permissions.mode() & 0o111 == 0 {
                println!("AppImage is not executable");
                return Ok(false);
            }
        }

        // TODO: Add more integrity checks (checksums, etc.)

        println!("AppImage integrity check passed");
        Ok(true)
    }

    /// Fix permissions on AppImage and home directory
    pub fn fix_permissions(capsule: &Capsule) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            // Fix AppImage permissions
            if capsule.appimage_path.exists() {
                let mut perms = fs::metadata(&capsule.appimage_path)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&capsule.appimage_path, perms)?;
                println!("Fixed AppImage permissions");
            }

            // Fix home directory permissions
            if capsule.home_path.exists() {
                Self::fix_dir_permissions(&capsule.home_path)?;
                println!("Fixed home directory permissions");
            }
        }

        #[cfg(not(unix))]
        {
            println!("Permission fixing not supported on this platform");
        }

        Ok(())
    }

    #[cfg(unix)]
    fn fix_dir_permissions(path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        for entry in walkdir::WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
            let metadata = entry.metadata()?;
            let mut perms = metadata.permissions();

            if metadata.is_dir() {
                perms.set_mode(0o755);
            } else {
                perms.set_mode(0o644);
            }

            fs::set_permissions(entry.path(), perms)?;
        }

        Ok(())
    }

    /// Complete capsule repair (all operations)
    pub fn full_repair(capsule: &mut Capsule) -> Result<()> {
        println!("Starting full capsule repair: {}", capsule.name);

        Self::verify_appimage(capsule)?;
        Self::fix_permissions(capsule)?;
        Self::clear_cache(capsule)?;
        Self::rebuild_prefix(capsule)?;
        Self::reset_runtime(capsule)?;

        println!("Full repair complete");
        Ok(())
    }
}
