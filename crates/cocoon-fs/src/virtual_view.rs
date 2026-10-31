use cocoon_core::{CocoonError, MountRule, MountType};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::debug;

use crate::scratchpad::Scratchpad;

/// Prepares the filesystem environment and redirects caches away from read-only mounts
pub struct VirtualView {
    pub scratchpad: Scratchpad,
    pub validated_mounts: Vec<MountRule>,
}

/// Guard that locks read-only mounts from being modified or deleted, and restores original access on drop
pub struct ReadOnlyGuard {
    #[cfg(windows)]
    locked_directories: Vec<PathBuf>,
    #[cfg(windows)]
    workspace_dir: Option<PathBuf>,
    #[cfg(not(windows))]
    modified_paths: Vec<(PathBuf, std::fs::Permissions)>,
}

impl Drop for ReadOnlyGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            for dir in &self.locked_directories {
                let _ = std::process::Command::new("icacls")
                    .arg(dir)
                    .arg("/remove:d")
                    .arg("*S-1-1-0")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }

            if let Some(ref ws) = self.workspace_dir {
                // Restore inherited permissions on workspace directory
                let _ = std::process::Command::new("icacls")
                    .arg(ws)
                    .arg("/inheritance:e")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }

        #[cfg(not(windows))]
        {
            for (path, perms) in &self.modified_paths {
                let _ = std::fs::set_permissions(path, perms.clone());
            }
        }
    }
}

impl VirtualView {
    pub fn setup(
        mounts: &[MountRule],
        custom_temp: Option<&Path>,
        custom_work: Option<&Path>,
        stage_copies: &[PathBuf],
    ) -> Result<Self, CocoonError> {
        let scratchpad = Scratchpad::with_paths(custom_temp, custom_work)?;

        // Stage requested copies into the workspace before execution
        for copy_source in stage_copies {
            scratchpad.stage_copy(copy_source)?;
        }

        let mut validated_mounts = Vec::new();

        for mount in mounts {
            let canonical = mount
                .host_path
                .canonicalize()
                .map_err(|_| CocoonError::PathNotFound(mount.host_path.clone()))?;

            debug!(
                "Validated mount {:?} -> {:?} ({:?})",
                mount.host_path, canonical, mount.mount_type
            );

            validated_mounts.push(MountRule {
                host_path: canonical,
                guest_path: mount.guest_path.clone(),
                mount_type: mount.mount_type.clone(),
            });
        }

        Ok(Self {
            scratchpad,
            validated_mounts,
        })
    }

    /// Recursively locks all files in ReadOnly mounts to read-only permissions,
    /// returning an RAII guard that restores original permissions when dropped.
    pub fn lock_read_only(&self) -> Result<ReadOnlyGuard, CocoonError> {
        #[cfg(windows)]
        {
            let mut locked = Vec::new();
            for mount in &self.validated_mounts {
                if mount.mount_type == MountType::ReadOnly {
                    // Apply Deny ACE for Delete, Delete Child, Write Data, and Append Data with inheritance
                    let status = std::process::Command::new("icacls")
                        .arg(&mount.host_path)
                        .arg("/deny")
                        .arg("*S-1-1-0:(OI)(CI)(DE,DC,WD,AD)")
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .map_err(CocoonError::Io)?;

                    if !status.success() {
                        return Err(CocoonError::SecurityViolation(format!(
                            "Failed to lock read-only mount: {:?}",
                            mount.host_path
                        )));
                    }
                    locked.push(mount.host_path.clone());
                }
            }

            // Ensure the designated workspace folder maintains write permissions
            let ws = self.scratchpad.workspace();
            let status_inh = std::process::Command::new("icacls")
                .arg(ws)
                .arg("/inheritance:r")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map_err(CocoonError::Io)?;

            if !status_inh.success() {
                return Err(CocoonError::SecurityViolation(format!(
                    "Failed to reset inheritance on workspace: {:?}",
                    ws
                )));
            }

            let status_grant = std::process::Command::new("icacls")
                .arg(ws)
                .arg("/grant")
                .arg("*S-1-1-0:(OI)(CI)F")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map_err(CocoonError::Io)?;

            if !status_grant.success() {
                return Err(CocoonError::SecurityViolation(format!(
                    "Failed to grant permissions on workspace: {:?}",
                    ws
                )));
            }

            Ok(ReadOnlyGuard {
                locked_directories: locked,
                workspace_dir: Some(ws.to_path_buf()),
            })
        }

        #[cfg(not(windows))]
        {
            let mut modified = Vec::new();
            for mount in &self.validated_mounts {
                if mount.mount_type == MountType::ReadOnly {
                    Self::protect_recursive(&mount.host_path, &mut modified)?;
                }
            }
            Ok(ReadOnlyGuard {
                modified_paths: modified,
            })
        }
    }

    #[cfg(not(windows))]
    fn protect_recursive(
        dir: &Path,
        modified: &mut Vec<(PathBuf, std::fs::Permissions)>,
    ) -> Result<(), CocoonError> {
        if dir.is_file() {
            if let Ok(meta) = std::fs::metadata(dir) {
                let orig = meta.permissions();
                if !orig.readonly() {
                    let mut ro_perms = orig.clone();
                    ro_perms.set_readonly(true);
                    if std::fs::set_permissions(dir, ro_perms).is_ok() {
                        modified.push((dir.to_path_buf(), orig));
                    }
                }
            }
            return Ok(());
        }

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        let orig = meta.permissions();
                        if !orig.readonly() {
                            let mut ro_perms = orig.clone();
                            ro_perms.set_readonly(true);
                            if std::fs::set_permissions(&path, ro_perms).is_ok() {
                                modified.push((path, orig));
                            }
                        }
                    }
                } else if path.is_dir() {
                    let _ = Self::protect_recursive(&path, modified);
                }
            }
        }

        Ok(())
    }

    /// Generates safe runtime environment variables redirecting temporary files
    /// and bytecode caches to the ephemeral scratchpad (prevents writing to read-only directories)
    pub fn safe_env_redirects(&self) -> HashMap<String, String> {
        let mut envs = HashMap::new();
        let scratch_str = self.scratchpad.root().to_string_lossy().to_string();

        // Redirect OS temp directories to the scratchpad
        envs.insert("TMP".into(), scratch_str.clone());
        envs.insert("TEMP".into(), scratch_str.clone());
        envs.insert("TMPDIR".into(), scratch_str.clone());

        // Redirect Python bytecode caching so it doesn't write __pycache__ into read-only mounts
        let pycache = self.scratchpad.root().join("pycache");
        let _ = std::fs::create_dir_all(&pycache);
        envs.insert(
            "PYTHONPYCACHEPREFIX".into(),
            pycache.to_string_lossy().to_string(),
        );
        envs.insert("PYTHONDONTWRITEBYTECODE".into(), "1".into());

        // Redirect Node npm cache
        let npm_cache = self.scratchpad.root().join("npm-cache");
        let _ = std::fs::create_dir_all(&npm_cache);
        envs.insert(
            "npm_config_cache".into(),
            npm_cache.to_string_lossy().to_string(),
        );

        // Redirect Cargo target directory
        let cargo_target = self.scratchpad.root().join("cargo-target");
        let _ = std::fs::create_dir_all(&cargo_target);
        envs.insert(
            "CARGO_TARGET_DIR".into(),
            cargo_target.to_string_lossy().to_string(),
        );

        // Redirect pip cache to scratchpad
        let pip_cache = self.scratchpad.root().join("pip-cache");
        let _ = std::fs::create_dir_all(&pip_cache);
        envs.insert(
            "PIP_CACHE_DIR".into(),
            pip_cache.to_string_lossy().to_string(),
        );

        // Security: Synthesize an isolated sandbox user profile inside the scratchpad
        // Prevents child processes from discovering and reading host credentials in ~/.aws, ~/.ssh, etc.
        let dummy_user = self.scratchpad.root().join("dummy_user");
        let _ = std::fs::create_dir_all(&dummy_user);
        let dummy_appdata = dummy_user.join("AppData");
        let dummy_roaming = dummy_appdata.join("Roaming");
        let dummy_local = dummy_appdata.join("Local");
        let _ = std::fs::create_dir_all(&dummy_roaming);
        let _ = std::fs::create_dir_all(&dummy_local);

        let dummy_user_str = dummy_user.to_string_lossy().to_string();
        envs.insert("USERPROFILE".into(), dummy_user_str.clone());
        envs.insert("HOME".into(), dummy_user_str.clone());
        envs.insert(
            "HOMEDRIVE".into(),
            dummy_user
                .components()
                .next()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .unwrap_or_else(|| "C:".into()),
        );
        envs.insert("HOMEPATH".into(), dummy_user_str.clone());
        envs.insert(
            "APPDATA".into(),
            dummy_roaming.to_string_lossy().to_string(),
        );
        envs.insert(
            "LOCALAPPDATA".into(),
            dummy_local.to_string_lossy().to_string(),
        );

        // Security: Disable user site packages and pip user installations inside sandbox
        envs.insert("PYTHONNOUSERSITE".into(), "1".into());
        envs.insert("PIP_NO_INDEX".into(), "1".into());

        // Neutralize host interpreter options that allow arbitrary code or hook injection
        envs.insert("NODE_OPTIONS".into(), "".into());
        envs.insert("RUBYOPT".into(), "".into());
        envs.insert("PERL5OPT".into(), "".into());

        // Security: Mitigate DLL preloading from Current Working Directory (CWD) on Windows
        envs.insert("CWDIllegalInDllSearch".into(), "0x2".into());

        // Offline toolchain configuration (enforces offline dependency resolution)
        envs.insert("CARGO_NET_OFFLINE".into(), "true".into());
        envs.insert("GOPROXY".into(), "off".into());
        envs.insert("GONOSUMDB".into(), "*".into());
        envs.insert("npm_config_offline".into(), "true".into());
        let empty_userbase = self.scratchpad.root().join("empty_userbase");
        let _ = std::fs::create_dir_all(&empty_userbase);
        envs.insert(
            "PYTHONUSERBASE".into(),
            empty_userbase.to_string_lossy().to_string(),
        );

        // Add workspace/Modules to PowerShell module search path
        let ps_modules = self.scratchpad.workspace().join("Modules");
        let _ = std::fs::create_dir_all(&ps_modules);
        let current_ps_path = std::env::var("PSModulePath").unwrap_or_default();
        let sep = if cfg!(windows) { ";" } else { ":" };
        envs.insert(
            "PSModulePath".into(),
            format!("{}{}{}", ps_modules.to_string_lossy(), sep, current_ps_path),
        );

        envs
    }

    /// Verifies if a given path is marked as ReadOnly in the configured mounts
    pub fn is_read_only(&self, path: &Path) -> bool {
        if let Ok(canon) = path.canonicalize() {
            for m in &self.validated_mounts {
                if m.mount_type == MountType::ReadOnly && canon.starts_with(&m.host_path) {
                    return true;
                }
            }
        }
        false
    }
}
