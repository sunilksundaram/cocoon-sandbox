pub mod linux;
#[cfg(windows)]
pub mod windows;

use cocoon_core::SandboxDriver;
use std::sync::Arc;

#[cfg(windows)]
pub use windows::WindowsJobObjectDriver;

pub use linux::LinuxNamespaceDriver;

/// Returns the default platform-native sandbox driver for the current operating system
pub fn get_default_driver() -> Arc<dyn SandboxDriver> {
    #[cfg(windows)]
    {
        Arc::new(WindowsJobObjectDriver::new())
    }

    #[cfg(target_os = "linux")]
    {
        Arc::new(LinuxNamespaceDriver::new())
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Arc::new(LinuxNamespaceDriver::new())
    }
}
