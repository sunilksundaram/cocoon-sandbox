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

/// Reason for process termination (if abnormally terminated)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminationReason {
    Timeout,
    MemoryExceeded,
    ProcessCountExceeded,
    SecurityViolation,
    KilledBySupervisor,
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
    /// Whether outbound network access is allowed (defaults to false for security)
    pub allow_network: bool,
    /// Maximum active processes allowed in the sandbox (defaults to 32)
    pub max_processes: Option<u32>,
    /// Maximum stdout/stderr output bytes captured before truncation (defaults to 10MB)
    pub max_output_bytes: Option<usize>,
    /// Maximum disk space (in MB) allowed for files created in scratchpad/workspace (defaults to 512MB)
    pub disk_limit_mb: Option<usize>,
    /// Whether GPU hardware access is permitted (defaults to true; false masks GPU drivers)
    pub allow_gpu: bool,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            timeout: Some(Duration::from_secs(30)),
            memory_limit_mb: None,
            cpu_limit_pct: Some(95),
            allow_network: false,
            max_processes: Some(32),
            max_output_bytes: Some(10 * 1024 * 1024),
            disk_limit_mb: Some(512),
            allow_gpu: true,
        }
    }
}

/// A live streaming chunk emitted during execution
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum StreamChunk {
    Stdout(String),
    Stderr(String),
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
    /// Optional channel sender to stream live output chunks in real time
    #[serde(skip)]
    pub stream_sender: Option<std::sync::mpsc::Sender<StreamChunk>>,
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
            stream_sender: None,
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
    /// Detailed reason if process was terminated abnormally
    #[serde(skip_serializing_if = "Option::is_none")]
    pub termination_reason: Option<TerminationReason>,
    /// True if stdout stream exceeded buffer limit and was truncated
    #[serde(default)]
    pub stdout_truncated: bool,
    /// True if stderr stream exceeded buffer limit and was truncated
    #[serde(default)]
    pub stderr_truncated: bool,
    /// Peak memory consumed in megabytes (if telemetry supported)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_memory_mb: Option<usize>,
}

impl ExecutionResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0 && !self.timed_out && !self.oom_killed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_limits_default() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.timeout, Some(Duration::from_secs(30)));
        assert_eq!(limits.memory_limit_mb, None);
        assert!(!limits.allow_network); // Default-deny network
        assert_eq!(limits.max_processes, Some(32));
        assert_eq!(limits.disk_limit_mb, Some(512));
    }

    #[test]
    fn test_mount_rule_constructors() {
        let ro = MountRule::read_only("/path/to/ro");
        assert_eq!(ro.mount_type, MountType::ReadOnly);
        assert_eq!(ro.host_path, PathBuf::from("/path/to/ro"));

        let sp = MountRule::scratchpad("/path/to/scratch");
        assert_eq!(sp.mount_type, MountType::Scratchpad);
        assert_eq!(sp.host_path, PathBuf::from("/path/to/scratch"));
    }

    #[test]
    fn test_execution_result_success_conditions() {
        let ok = ExecutionResult {
            exit_code: 0,
            stdout: "ok".into(),
            stderr: String::new(),
            duration_ms: 10.0,
            timed_out: false,
            oom_killed: false,
            termination_reason: None,
            stdout_truncated: false,
            stderr_truncated: false,
            peak_memory_mb: None,
        };
        assert!(ok.success());

        let nonzero = ExecutionResult {
            exit_code: 1,
            ..ok.clone()
        };
        assert!(!nonzero.success());

        let timed_out = ExecutionResult {
            timed_out: true,
            ..ok.clone()
        };
        assert!(!timed_out.success());

        let oom = ExecutionResult {
            oom_killed: true,
            ..ok
        };
        assert!(!oom.success());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let config = SandboxConfig::default();
        let json = serde_json::to_string(&config).expect("Must serialize");
        let deserialized: SandboxConfig = serde_json::from_str(&json).expect("Must deserialize");
        assert_eq!(deserialized.limits.timeout, Some(Duration::from_secs(30)));
    }
}
