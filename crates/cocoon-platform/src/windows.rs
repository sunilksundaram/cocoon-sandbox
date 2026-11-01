use cocoon_core::{
    CocoonError, ExecutionResult, ResourceLimits, SandboxConfig, SandboxDriver, TerminationReason,
};
use cocoon_fs::VirtualView;
use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::FreeSid;
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::NORMAL_PRIORITY_CLASS;

#[allow(clippy::upper_case_acronyms)]
type PSID = *mut std::ffi::c_void;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicUIRestrictions,
    JobObjectCpuRateControlInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE,
    JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PRIORITY_CLASS,
    JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOB_OBJECT_UILIMIT_DESKTOP,
    JOB_OBJECT_UILIMIT_DISPLAYSETTINGS, JOB_OBJECT_UILIMIT_EXITWINDOWS,
    JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_HANDLES, JOB_OBJECT_UILIMIT_READCLIPBOARD,
    JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS, JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
};

/// Query the host physical memory in Megabytes.
pub fn get_host_physical_memory_mb() -> Option<usize> {
    unsafe {
        let mut status: MEMORYSTATUSEX = std::mem::zeroed();
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut status) != 0 {
            Some((status.ullTotalPhys / (1024 * 1024)) as usize)
        } else {
            None
        }
    }
}

#[derive(Default)]
pub struct WindowsJobObjectDriver;

impl WindowsJobObjectDriver {
    pub fn new() -> Self {
        Self
    }

    /// Configures a Windows Job Object with memory, process caps, and CPU rate limits
    fn setup_job_object(&self, limits: &ResourceLimits) -> Result<HANDLE, CocoonError> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(CocoonError::Platform(
                    "Failed to create Windows Job Object".into(),
                ));
            }

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            let mut limit_flags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;

            // Auto-detect host physical memory to enforce a strict 95% safety ceiling.
            // This prevents guest code from paging thrashing or blue-screening the host.
            let host_phys_mb = get_host_physical_memory_mb().unwrap_or(4096);
            let max_safe_mem_mb = (host_phys_mb * 95) / 100;

            let effective_mem_mb = match limits.memory_limit_mb {
                Some(requested_mb) => requested_mb.min(max_safe_mem_mb),
                None => max_safe_mem_mb,
            };

            limit_flags |= JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            let mem_bytes = effective_mem_mb * 1024 * 1024;
            info.JobMemoryLimit = mem_bytes;
            info.ProcessMemoryLimit = mem_bytes;

            // Lock process priority to NORMAL_PRIORITY_CLASS to prevent child processes from elevating
            // to REALTIME_PRIORITY_CLASS or HIGH_PRIORITY_CLASS and starving the host OS scheduler.
            limit_flags |= JOB_OBJECT_LIMIT_PRIORITY_CLASS;
            info.BasicLimitInformation.PriorityClass = NORMAL_PRIORITY_CLASS;

            // Enforce active process limit (mitigates fork bombs and handle exhaustion)
            if let Some(max_procs) = limits.max_processes {
                limit_flags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
                info.BasicLimitInformation.ActiveProcessLimit = max_procs;
            }

            info.BasicLimitInformation.LimitFlags = limit_flags;

            let res = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );

            if res == 0 {
                CloseHandle(job);
                return Err(CocoonError::Platform(
                    "Failed to apply Windows Job Object limits".into(),
                ));
            }

            // Enforce CPU rate limiting (capped at 95% MAX of hardware to guarantee host OS responsiveness)
            let cpu_pct = limits.cpu_limit_pct.unwrap_or(95).clamp(1, 95);
            let mut cpu_info: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = std::mem::zeroed();
            cpu_info.ControlFlags =
                JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
            cpu_info.Anonymous.CpuRate = cpu_pct * 100;

            let res_cpu = SetInformationJobObject(
                job,
                JobObjectCpuRateControlInformation,
                &cpu_info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
            );

            if res_cpu == 0 {
                CloseHandle(job);
                return Err(CocoonError::Platform(
                    "Failed to apply Windows Job Object CPU rate limits".into(),
                ));
            }

            // Enforce UI, Clipboard, and Desktop restrictions (prevents clipboard theft, shatter attacks, desktop injection)
            let mut ui_restrictions: JOBOBJECT_BASIC_UI_RESTRICTIONS = std::mem::zeroed();
            ui_restrictions.UIRestrictionsClass = JOB_OBJECT_UILIMIT_HANDLES
                | JOB_OBJECT_UILIMIT_READCLIPBOARD
                | JOB_OBJECT_UILIMIT_WRITECLIPBOARD
                | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
                | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
                | JOB_OBJECT_UILIMIT_GLOBALATOMS
                | JOB_OBJECT_UILIMIT_DESKTOP
                | JOB_OBJECT_UILIMIT_EXITWINDOWS;

            let res_ui = SetInformationJobObject(
                job,
                JobObjectBasicUIRestrictions,
                &ui_restrictions as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
            );

            if res_ui == 0 {
                CloseHandle(job);
                return Err(CocoonError::Platform(
                    "Failed to apply Windows Job Object UI restrictions".into(),
                ));
            }

            Ok(job)
        }
    }
}

impl SandboxDriver for WindowsJobObjectDriver {
    fn name(&self) -> &'static str {
        "windows-job-object"
    }

    fn is_available(&self) -> bool {
        cfg!(windows)
    }

    fn execute(&self, config: &SandboxConfig) -> Result<ExecutionResult, CocoonError> {
        let start_time = Instant::now();

        // 1. Setup virtual filesystem view and safe redirects
        let vfs = VirtualView::setup(
            &config.mounts,
            config.temp_dir.as_deref(),
            config.working_dir.as_deref(),
            &config.stage_copies,
        )?;
        let _ro_guard = vfs.lock_read_only()?;
        let safe_env = vfs.safe_env_redirects();

        // 2. Configure Windows Job Object
        let job_handle = self.setup_job_object(&config.limits)?;

        // RAII guard to always close job handle
        struct JobGuard(HANDLE);
        impl Drop for JobGuard {
            fn drop(&mut self) {
                unsafe {
                    if !self.0.is_null() {
                        CloseHandle(self.0);
                    }
                }
            }
        }
        let _guard = JobGuard(job_handle);

        // 3. Build child command
        let mut cmd = Command::new(&config.executable);
        cmd.args(&config.args);

        // Apply working directory
        if let Some(ref wd) = config.working_dir {
            cmd.current_dir(wd);
        } else if !config.stage_copies.is_empty() {
            cmd.current_dir(vfs.scratchpad.workspace());
        } else if let Some(first_mount) = config.mounts.first().filter(|m| m.host_path.is_dir()) {
            cmd.current_dir(&first_mount.host_path);
        } else {
            cmd.current_dir(vfs.scratchpad.workspace());
        }

        // Apply environment variables
        if !config.inherit_env {
            cmd.env_clear();
            // Retain minimal critical Windows environment variables (USERPROFILE/APPDATA omitted for credential security)
            for var in &[
                "SystemRoot",
                "SystemDrive",
                "windir",
                "PATH",
                "PATHEXT",
                "COMSPEC",
                "GOROOT",
                "GOPATH",
                "JAVA_HOME",
                "CARGO_HOME",
                "RUSTUP_HOME",
                "BUN_INSTALL",
                "DENO_DIR",
            ] {
                if let Ok(val) = std::env::var(var) {
                    cmd.env(var, val);
                }
            }
        }

        // Explicitly strip host credential and IPC pipe environment variables even if inherit_env is enabled
        for dangerous_var in &[
            "DOCKER_HOST",
            "SSH_AUTH_SOCK",
            "WSL_DISTRO_NAME",
            "WSL_INTEROP",
            "GPG_AGENT_INFO",
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "GIT_ASKPASS",
            "AZURE_CONFIG_DIR",
            "KUBECONFIG",
        ] {
            cmd.env_remove(dangerous_var);
        }

        // Enforce network isolation in environment if disabled
        if !config.limits.allow_network {
            cmd.env("HTTP_PROXY", "http://127.0.0.1:0");
            cmd.env("HTTPS_PROXY", "http://127.0.0.1:0");
            cmd.env("ALL_PROXY", "socks5://127.0.0.1:0");
            cmd.env("no_proxy", "");
        }

        // Enforce GPU isolation or safety guards
        if !config.limits.allow_gpu {
            // Completely detach GPU hardware drivers and runtimes from child process
            cmd.env("CUDA_VISIBLE_DEVICES", "");
            cmd.env("ROCR_VISIBLE_DEVICES", "");
            cmd.env("HIP_VISIBLE_DEVICES", "");
            cmd.env("OPENCL_DEVICE_NAME", "");
            cmd.env("GPU_DEVICE_ORDINAL", "");
            cmd.env("NVIDIA_VISIBLE_DEVICES", "void");
            cmd.env("ONEAPI_DEVICE_SELECTOR", "none");
            cmd.env("SYCL_DEVICE_FILTER", "none");
        } else {
            // Apply PyTorch VRAM allocator fragmentation guard to prevent rapid out-of-control memory spikes
            cmd.env("PYTORCH_CUDA_ALLOC_CONF", "max_split_size_mb:128");
        }

        // Suppress JIT debugger attachments and crash dialog popups (prevents hangs and storage flooding)
        cmd.env("COMPlus_DbgJitDebugLaunchSetting", "0");
        cmd.env("DOTNET_DbgJitDebugLaunchSetting", "0");

        // Apply VFS cache redirects (prevents modifying read-only folders for bytecode, temp, etc.)
        for (k, v) in safe_env {
            cmd.env(k, v);
        }

        // Apply user-defined environment variables
        for (k, v) in &config.env {
            cmd.env(k, v);
        }

        cmd.stdin(if config.stdin_data.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Spawn process
        debug!("Spawning sandboxed process {:?}", config.executable);
        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CocoonError::RuntimeNotFound(config.executable.to_string_lossy().to_string())
            } else {
                CocoonError::Io(e)
            }
        })?;

        // Assign child process to Job Object
        unsafe {
            let child_handle = child.as_raw_handle() as HANDLE;
            let assign_res = AssignProcessToJobObject(job_handle, child_handle);
            if assign_res == 0 {
                let _ = child.kill();
                return Err(CocoonError::Platform(
                    "Failed to assign process to Windows Job Object".into(),
                ));
            }
        }

        // Write stdin if requested
        if let Some(ref data) = config.stdin_data {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(data);
            }
        }

        // Setup concurrent stdout & stderr readers with buffer caps (prevents memory DoS)
        let max_output = config.limits.max_output_bytes.unwrap_or(10 * 1024 * 1024);
        let mut stdout_handle = child.stdout.take();
        let mut stderr_handle = child.stderr.take();

        let (stdout_tx, stdout_rx) = mpsc::channel();
        let (stderr_tx, stderr_rx) = mpsc::channel();

        let stream_tx_out = config.stream_sender.clone();
        let stream_tx_err = config.stream_sender.clone();

        if let Some(mut out) = stdout_handle.take() {
            thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                let mut truncated = false;
                loop {
                    match out.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Some(ref stx) = stream_tx_out {
                                let _ = stx.send(cocoon_core::StreamChunk::Stdout(
                                    String::from_utf8_lossy(&chunk[..n]).to_string(),
                                ));
                            }
                            if buf.len() + n <= max_output {
                                buf.extend_from_slice(&chunk[..n]);
                            } else {
                                let rem = max_output.saturating_sub(buf.len());
                                if rem > 0 {
                                    buf.extend_from_slice(&chunk[..rem]);
                                }
                                truncated = true;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = stdout_tx.send((buf, truncated));
            });
        } else {
            let _ = stdout_tx.send((Vec::new(), false));
        }

        if let Some(mut err) = stderr_handle.take() {
            thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                let mut truncated = false;
                loop {
                    match err.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Some(ref stx) = stream_tx_err {
                                let _ = stx.send(cocoon_core::StreamChunk::Stderr(
                                    String::from_utf8_lossy(&chunk[..n]).to_string(),
                                ));
                            }
                            if buf.len() + n <= max_output {
                                buf.extend_from_slice(&chunk[..n]);
                            } else {
                                let rem = max_output.saturating_sub(buf.len());
                                if rem > 0 {
                                    buf.extend_from_slice(&chunk[..rem]);
                                }
                                truncated = true;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = stderr_tx.send((buf, truncated));
            });
        } else {
            let _ = stderr_tx.send((Vec::new(), false));
        }

        // Wait with timeout and atomic job tree termination
        let timeout = config.limits.timeout;
        let mut timed_out = false;
        let mut termination_reason = None;
        let mut exit_code = -1;

        if let Some(duration) = timeout {
            let poll_interval = Duration::from_millis(10);
            let deadline = Instant::now() + duration;

            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        exit_code = status.code().unwrap_or(-1);
                        break;
                    }
                    Ok(None) => {
                        if Instant::now() >= deadline {
                            timed_out = true;
                            termination_reason = Some(TerminationReason::Timeout);
                            warn!(
                                target: "cocoon::security",
                                event = "execution_timeout",
                                executable = %config.executable.display(),
                                timeout_ms = duration.as_millis(),
                                "Process exceeded execution timeout limit; terminating entire job object tree"
                            );
                            unsafe {
                                // Atomically terminate all processes in the job object tree
                                TerminateJobObject(job_handle, 1);
                            }
                            let _ = child.wait();
                            break;
                        }
                        thread::sleep(poll_interval);
                    }
                    Err(e) => {
                        unsafe {
                            TerminateJobObject(job_handle, 1);
                        }
                        return Err(CocoonError::Io(e));
                    }
                }
            }
        } else {
            match child.wait() {
                Ok(status) => exit_code = status.code().unwrap_or(-1),
                Err(e) => return Err(CocoonError::Io(e)),
            }
        }

        let (stdout_bytes, stdout_truncated) = stdout_rx.recv().unwrap_or_default();
        let (stderr_bytes, stderr_truncated) = stderr_rx.recv().unwrap_or_default();
        let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;

        // Query Job Object telemetry (PeakJobMemoryUsed, etc.) before handle closure
        let mut peak_memory_mb = None;
        let mut oom_killed = false;
        unsafe {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            let mut return_len = 0u32;
            let query_res = QueryInformationJobObject(
                job_handle,
                JobObjectExtendedLimitInformation,
                &mut info as *mut _ as *mut _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                &mut return_len,
            );
            if query_res != 0 && info.PeakJobMemoryUsed > 0 {
                let mb = (info.PeakJobMemoryUsed / (1024 * 1024)) as usize;
                peak_memory_mb = Some(mb);
                if let Some(mem_limit) = config.limits.memory_limit_mb {
                    if mb >= mem_limit || exit_code as u32 == 0xC000_0017 {
                        oom_killed = true;
                        if termination_reason.is_none() {
                            termination_reason = Some(TerminationReason::MemoryExceeded);
                        }
                    }
                }
            }
        }

        // Security check: verify no junctions or symlinks inside workspace escape to host files
        vfs.scratchpad.verify_no_breakouts()?;

        // Security check: enforce disk write quota if configured
        if let Some(disk_mb) = config.limits.disk_limit_mb {
            vfs.scratchpad.verify_disk_quota(disk_mb)?;
        }

        Ok(ExecutionResult {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout_bytes).to_string(),
            stderr: String::from_utf8_lossy(&stderr_bytes).to_string(),
            duration_ms,
            timed_out,
            oom_killed,
            termination_reason,
            stdout_truncated,
            stderr_truncated,
            peak_memory_mb,
        })
    }
}

/// RAII Guard managing Windows AppContainer profile creation, SID lifecycle, and cleanup
pub struct AppContainerProfile {
    pub name: Vec<u16>,
    pub sid: PSID,
}

impl AppContainerProfile {
    /// Creates an ephemeral AppContainer with 0 capabilities (blocks all network access at kernel level)
    pub fn create(profile_name: &str) -> Result<Self, CocoonError> {
        let name_wide: Vec<u16> = profile_name.encode_utf16().chain(Some(0)).collect();
        let mut sid: PSID = std::ptr::null_mut();

        unsafe {
            let hr = CreateAppContainerProfile(
                name_wide.as_ptr(),
                name_wide.as_ptr(),
                name_wide.as_ptr(),
                std::ptr::null(), // 0 capabilities = Zero Network
                0,
                &mut sid,
            );
            // S_OK is 0
            if hr < 0 {
                return Err(CocoonError::Platform(format!(
                    "Failed to create AppContainer profile (HRESULT: 0x{:08X})",
                    hr
                )));
            }
        }
        Ok(Self {
            name: name_wide,
            sid,
        })
    }
}

impl Drop for AppContainerProfile {
    fn drop(&mut self) {
        unsafe {
            if !self.sid.is_null() {
                FreeSid(self.sid);
            }
            DeleteAppContainerProfile(self.name.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_container_profile_creation() {
        let profile = AppContainerProfile::create("CocoonTestAppContainerProfile");
        assert!(profile.is_ok(), "Failed: {:?}", profile.err());
    }
}
