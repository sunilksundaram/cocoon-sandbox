use cocoon_core::CocoonError;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tracing::{debug, warn};

enum TempDirHolder {
    Ephemeral(TempDir),
    Persistent(PathBuf),
}

impl TempDirHolder {
    fn path(&self) -> &Path {
        match self {
            TempDirHolder::Ephemeral(t) => t.path(),
            TempDirHolder::Persistent(p) => p.as_path(),
        }
    }
}

/// Manages temporary and workspace directories for execution
pub struct Scratchpad {
    temp_holder: TempDirHolder,
    workspace_dir: PathBuf,
}

impl Scratchpad {
    /// Creates a new ephemeral scratchpad
    pub fn new() -> Result<Self, CocoonError> {
        Self::with_paths(None, None)
    }

    /// Creates a scratchpad with optional custom temp and workspace paths
    pub fn with_paths(
        custom_temp: Option<&Path>,
        custom_work: Option<&Path>,
    ) -> Result<Self, CocoonError> {
        let temp_holder = if let Some(p) = custom_temp {
            std::fs::create_dir_all(p).map_err(CocoonError::Io)?;
            TempDirHolder::Persistent(p.to_path_buf())
        } else {
            // Sweep stale scratchpads older than 2 hours from previous crashed runs
            Self::cleanup_stale_scratchpads();

            let temp_dir = tempfile::Builder::new()
                .prefix("cocoon-scratch-")
                .tempdir()
                .map_err(CocoonError::Io)?;
            TempDirHolder::Ephemeral(temp_dir)
        };

        let workspace_dir = if let Some(w) = custom_work {
            std::fs::create_dir_all(w).map_err(CocoonError::Io)?;
            w.canonicalize().unwrap_or_else(|_| w.to_path_buf())
        } else {
            let ws = temp_holder.path().join("workspace");
            std::fs::create_dir_all(&ws).map_err(CocoonError::Io)?;
            ws
        };

        Ok(Self {
            temp_holder,
            workspace_dir,
        })
    }

    /// Garbage collection: Purges stale ephemeral scratchpads left by previous crashed runs
    pub fn cleanup_stale_scratchpads() {
        let temp = std::env::temp_dir();
        if let Ok(entries) = std::fs::read_dir(temp) {
            let threshold = std::time::Duration::from_secs(2 * 3600); // 2 hours
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("cocoon-scratch-") {
                    let path = entry.path();
                    if let Ok(meta) = std::fs::metadata(&path) {
                        if let Ok(modified) = meta.modified() {
                            if let Ok(elapsed) = modified.elapsed() {
                                if elapsed > threshold {
                                    let _ = std::fs::remove_dir_all(&path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Verifies that no symlinks or Windows junctions inside the workspace break out to host files
    pub fn verify_no_breakouts(&self) -> Result<(), CocoonError> {
        let root = self
            .workspace_dir
            .canonicalize()
            .unwrap_or_else(|_| self.workspace_dir.clone());
        Self::check_breakouts_recursive(&self.workspace_dir, &root)
    }

    /// Verifies that total disk usage in the scratchpad and workspace does not exceed the quota limit
    pub fn verify_disk_quota(&self, limit_mb: usize) -> Result<(), CocoonError> {
        let max_bytes = (limit_mb * 1024 * 1024) as u64;
        let total_bytes = Self::compute_dir_size(self.root())?;
        if total_bytes > max_bytes {
            warn!(
                target: "cocoon::security",
                event = "disk_quota_exceeded",
                consumed_bytes = total_bytes,
                limit_mb = limit_mb,
                "Disk quota exceeded in sandbox scratchpad: {:.1} MB > {} MB",
                total_bytes as f64 / (1024.0 * 1024.0),
                limit_mb
            );
            return Err(CocoonError::SecurityViolation(format!(
                "Storage quota exceeded: {:.1} MB consumed (limit: {} MB)",
                total_bytes as f64 / (1024.0 * 1024.0),
                limit_mb
            )));
        }
        Ok(())
    }

    fn compute_dir_size(dir: &Path) -> Result<u64, CocoonError> {
        let mut total = 0;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(meta) = std::fs::symlink_metadata(&path) {
                    if meta.is_file() {
                        total += meta.len();
                    } else if meta.is_dir() && !meta.file_type().is_symlink() {
                        total += Self::compute_dir_size(&path)?;
                    }
                }
            }
        }
        Ok(total)
    }

    fn check_breakouts_recursive(dir: &Path, root: &Path) -> Result<(), CocoonError> {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(meta) = std::fs::symlink_metadata(&path) {
                    if meta.file_type().is_symlink() {
                        if let Ok(target) = std::fs::read_link(&path) {
                            let resolved = if target.is_relative() {
                                path.parent().unwrap_or(root).join(&target)
                            } else {
                                target
                            };
                            if let Ok(canon) = resolved.canonicalize() {
                                if !canon.starts_with(root) {
                                    warn!(
                                        target: "cocoon::security",
                                        event = "security_violation",
                                        violation_type = "symlink_escape",
                                        source_path = %path.display(),
                                        target_path = %canon.display(),
                                        "Detected symlink breakout pointing outside workspace"
                                    );
                                    return Err(CocoonError::SecurityViolation(format!(
                                        "Detected symlink breakout pointing outside workspace: {:?} -> {:?}",
                                        path, canon
                                    )));
                                }
                            }
                        }
                    }

                    // Security check: verify that this path's canonical resolution resides strictly within the workspace
                    // This catches symlinks, NTFS junctions, hardlinks, and reparse points escaping the workspace
                    if let Ok(canon) = path.canonicalize() {
                        if !canon.starts_with(root) {
                            warn!(
                                target: "cocoon::security",
                                event = "security_violation",
                                violation_type = "path_escape",
                                source_path = %path.display(),
                                target_path = %canon.display(),
                                "Detected path escape pointing outside workspace"
                            );
                            return Err(CocoonError::SecurityViolation(format!(
                                "Detected path escape pointing outside workspace: {:?} -> {:?}",
                                path, canon
                            )));
                        }
                    }

                    if path.is_dir() && !meta.file_type().is_symlink() {
                        Self::check_breakouts_recursive(&path, root)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Root path of the temporary directory (for TMP/TEMP/caching)
    pub fn root(&self) -> &Path {
        self.temp_holder.path()
    }

    /// Dedicated workspace directory for creating, modifying, and running code
    pub fn workspace(&self) -> &Path {
        &self.workspace_dir
    }

    /// Creates a temporary subdirectory inside the temp directory
    pub fn create_subdir<P: AsRef<Path>>(&self, subpath: P) -> Result<PathBuf, CocoonError> {
        let path = self.temp_holder.path().join(subpath);
        std::fs::create_dir_all(&path).map_err(CocoonError::Io)?;
        Ok(path)
    }

    /// Copies a file or directory recursively into the workspace directory
    pub fn stage_copy<P: AsRef<Path>>(&self, source: P) -> Result<PathBuf, CocoonError> {
        let src = source.as_ref();
        if !src.exists() {
            return Err(CocoonError::PathNotFound(src.to_path_buf()));
        }

        let file_name = src
            .file_name()
            .ok_or_else(|| CocoonError::Config(format!("Invalid copy source: {:?}", src)))?;

        let destination = self.workspace_dir.join(file_name);
        debug!("Staging copy from {:?} to {:?}", src, destination);

        Self::copy_recursive_internal(src, &destination)?;
        Ok(destination)
    }

    fn copy_recursive_internal(src: &Path, dst: &Path) -> Result<(), CocoonError> {
        if src.is_dir() {
            std::fs::create_dir_all(dst).map_err(CocoonError::Io)?;
            for entry in std::fs::read_dir(src).map_err(CocoonError::Io)? {
                let entry = entry.map_err(CocoonError::Io)?;
                let child_src = entry.path();
                let child_dst = dst.join(entry.file_name());
                Self::copy_recursive_internal(&child_src, &child_dst)?;
            }
        } else {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(CocoonError::Io)?;
            }
            std::fs::copy(src, dst).map_err(CocoonError::Io)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scratchpad_ephemeral_creation() {
        let scratchpad = Scratchpad::new().expect("Failed to create scratchpad");
        assert!(scratchpad.root().exists());
        assert!(scratchpad.workspace().exists());
    }

    #[test]
    fn test_scratchpad_stage_copy_file() {
        let scratchpad = Scratchpad::new().expect("Failed to create scratchpad");
        let temp_src = tempfile::NamedTempFile::new().expect("Failed to create temp source");
        std::fs::write(temp_src.path(), "hello cocoon test").expect("Failed to write");

        let staged = scratchpad
            .stage_copy(temp_src.path())
            .expect("Stage copy should succeed");
        assert!(staged.exists());
        let content = std::fs::read_to_string(&staged).expect("Read staged file");
        assert_eq!(content, "hello cocoon test");
    }

    #[test]
    fn test_scratchpad_stage_copy_nonexistent() {
        let scratchpad = Scratchpad::new().expect("Failed to create scratchpad");
        let result = scratchpad.stage_copy(Path::new("definitely_nonexistent_file_12345"));
        assert!(result.is_err());
    }
}
