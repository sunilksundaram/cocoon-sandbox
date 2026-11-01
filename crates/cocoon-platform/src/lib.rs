pub mod linux;
pub mod macos;
#[cfg(windows)]
pub mod windows;

use cocoon_core::SandboxDriver;
use std::sync::Arc;

pub use linux::LinuxNamespaceDriver;
pub use macos::MacOsSeatbeltDriver;
#[cfg(windows)]
pub use windows::WindowsJobObjectDriver;

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

    #[cfg(target_os = "macos")]
    {
        Arc::new(MacOsSeatbeltDriver::new())
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        Arc::new(LinuxNamespaceDriver::new())
    }
}
