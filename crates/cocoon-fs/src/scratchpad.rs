use cocoon_core::CocoonError;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tracing::debug;

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
