use crate::config::{ExecutionResult, SandboxConfig};
use crate::error::CocoonError;

/// Core interface for platform-specific sandbox implementations
pub trait SandboxDriver: Send + Sync {
    /// Returns the human-readable name of the driver (e.g. "windows-job-object", "linux-namespace")
    fn name(&self) -> &'static str;

    /// Checks if this sandbox driver is supported and operational on the current system
    fn is_available(&self) -> bool;

    /// Executes the sandboxed process with the provided configuration
    fn execute(&self, config: &SandboxConfig) -> Result<ExecutionResult, CocoonError>;
}
