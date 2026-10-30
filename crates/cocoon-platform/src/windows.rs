use cocoon_core::{CocoonError, ExecutionResult, ResourceLimits, SandboxConfig, SandboxDriver};
use cocoon_fs::VirtualView;
use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tracing::debug;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

#[derive(Default)]
pub struct WindowsJobObjectDriver;

impl WindowsJobObjectDriver {
    pub fn new() -> Self {
        Self
    }

    /// Configures a Windows Job Object with memory caps and process isolation
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

            if let Some(mem_mb) = limits.memory_limit_mb {
                limit_flags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
                info.JobMemoryLimit = mem_mb * 1024 * 1024;
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
        } else if let Some(first_mount) = config.mounts.first() {
            cmd.current_dir(&first_mount.host_path);
        } else {
            cmd.current_dir(vfs.scratchpad.workspace());
        }

        // Apply environment variables
        if !config.inherit_env {
            cmd.env_clear();
            // Retain minimal critical Windows environment variables
            for var in &[
                "SystemRoot",
                "SystemDrive",
                "windir",
                "PATH",
                "PATHEXT",
                "COMSPEC",
                "USERPROFILE",
                "LOCALAPPDATA",
                "APPDATA",
                "HOMEDRIVE",
                "HOMEPATH",
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

        // Setup concurrent stdout & stderr readers
        let mut stdout_handle = child.stdout.take();
        let mut stderr_handle = child.stderr.take();

        let (stdout_tx, stdout_rx) = mpsc::channel();
        let (stderr_tx, stderr_rx) = mpsc::channel();

        if let Some(mut out) = stdout_handle.take() {
            thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = out.read_to_end(&mut buf);
                let _ = stdout_tx.send(buf);
            });
        } else {
            let _ = stdout_tx.send(Vec::new());
        }

        if let Some(mut err) = stderr_handle.take() {
            thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = err.read_to_end(&mut buf);
                let _ = stderr_tx.send(buf);
            });
        } else {
            let _ = stderr_tx.send(Vec::new());
        }

        // Wait with timeout
        let timeout = config.limits.timeout;
        let mut timed_out = false;
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
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                        thread::sleep(poll_interval);
                    }
                    Err(e) => {
                        let _ = child.kill();
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

        let stdout_bytes = stdout_rx.recv().unwrap_or_default();
        let stderr_bytes = stderr_rx.recv().unwrap_or_default();
        let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;

        Ok(ExecutionResult {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout_bytes).to_string(),
            stderr: String::from_utf8_lossy(&stderr_bytes).to_string(),
            duration_ms,
            timed_out,
            oom_killed: false,
        })
    }
}
