use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CocoonError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Platform sandbox error: {0}")]
    Platform(String),

    #[error("Execution timed out after {0:?}")]
    Timeout(std::time::Duration),

    #[error("Memory limit exceeded: {0} MB")]
    OutOfMemory(usize),

    #[error("Path not found: {0}")]
    PathNotFound(PathBuf),

    #[error("Runtime not found: '{0}'. Ensure it is installed and in PATH.")]
    RuntimeNotFound(String),

    #[error("Security violation: {0}")]
    SecurityViolation(String),

    #[error("Configuration error: {0}")]
    Config(String),
}
