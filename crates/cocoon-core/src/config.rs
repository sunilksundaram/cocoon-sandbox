use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Mount access mode
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MountType {
    /// Read-only access to host directory. Modifications strictly prevented.
    ReadOnly,
    /// Ephemeral in-memory or temporary scratchpad directory.
    Scratchpad,
    /// Explicit read-write directory (isolated to specified path).
    ReadWrite,
}

/// A mount rule mapping a host path into the sandbox environment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountRule {
    pub host_path: PathBuf,
    pub guest_path: Option<PathBuf>,
    pub mount_type: MountType,
}

impl MountRule {
    pub fn read_only<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            host_path: path.into(),
            guest_path: None,
            mount_type: MountType::ReadOnly,
        }
    }

    pub fn scratchpad<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            host_path: path.into(),
            guest_path: None,
            mount_type: MountType::Scratchpad,
        }
    }
}

/// Resource constraints imposed on the guest process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum wall-clock time the process is allowed to run (defaults to 30s)
    pub timeout: Option<Duration>,
    /// Maximum memory (RAM) allocation in megabytes
    pub memory_limit_mb: Option<usize>,
    /// CPU limit percentage (1..=100)
    pub cpu_limit_pct: Option<u32>,
    /// Whether outbound network access is allowed
    pub allow_network: bool,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            timeout: Some(Duration::from_secs(30)),
            memory_limit_mb: None,
            cpu_limit_pct: None,
            allow_network: true,
        }
    }
}

/// Specification for a sandbox execution session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Executable binary to launch
    pub executable: PathBuf,
    /// Command-line arguments
    pub args: Vec<String>,
    /// Working directory inside the sandbox where commands execute and files can be created/modified
    pub working_dir: Option<PathBuf>,
    /// Optional custom temporary directory for scratch files and caches (sets TMP/TEMP/TMPDIR)
    pub temp_dir: Option<PathBuf>,
    /// Files or directories to copy into the working directory prior to execution
    pub stage_copies: Vec<PathBuf>,
    /// Environment variables to set
    pub env: HashMap<String, String>,
    /// Whether to inherit host environment variables (filtered for safety)
    pub inherit_env: bool,
    /// Mounted filesystem paths
    pub mounts: Vec<MountRule>,
    /// Resource and execution limits
    pub limits: ResourceLimits,
    /// Optional stdin input to feed into the process
    pub stdin_data: Option<Vec<u8>>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            executable: PathBuf::new(),
            args: Vec::new(),
            working_dir: None,
            temp_dir: None,
            stage_copies: Vec::new(),
            env: HashMap::new(),
            inherit_env: false,
            mounts: Vec::new(),
            limits: ResourceLimits::default(),
            stdin_data: None,
        }
    }
}

/// Result of executing a command inside the sandbox
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Process exit code (or -1 if terminated abnormally)
    pub exit_code: i32,
    /// Standard output stream as UTF-8 string
    pub stdout: String,
    /// Standard error stream as UTF-8 string
    pub stderr: String,
    /// Total wall-clock duration of the execution in milliseconds
    pub duration_ms: f64,
    /// True if process was forcibly killed due to exceeding timeout limit
    pub timed_out: bool,
    /// True if process was killed due to exceeding memory limit
    pub oom_killed: bool,
}

impl ExecutionResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0 && !self.timed_out && !self.oom_killed
    }
}
