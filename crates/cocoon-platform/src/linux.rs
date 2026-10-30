use cocoon_core::{CocoonError, ExecutionResult, MountType, SandboxConfig, SandboxDriver};
use cocoon_fs::VirtualView;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Linux Sandbox Driver using Bubblewrap or Linux User Namespaces
#[derive(Default)]
pub struct LinuxNamespaceDriver;

impl LinuxNamespaceDriver {
    pub fn new() -> Self {
        Self
    }

    /// Check if `bwrap` (Bubblewrap) is present on the system
    fn has_bwrap(&self) -> bool {
        Command::new("bwrap")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

impl SandboxDriver for LinuxNamespaceDriver {
    fn name(&self) -> &'static str {
        "linux-namespace"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "linux")
    }

    fn execute(&self, config: &SandboxConfig) -> Result<ExecutionResult, CocoonError> {
        let start_time = Instant::now();
        let vfs = VirtualView::setup(
            &config.mounts,
            config.temp_dir.as_deref(),
            config.working_dir.as_deref(),
            &config.stage_copies,
        )?;
        let safe_env = vfs.safe_env_redirects();

        // If bwrap is available, run unprivileged kernel-level sandbox
        let mut cmd = if self.has_bwrap() {
            let mut b = Command::new("bwrap");
            b.arg("--unshare-all")
                .arg("--die-with-parent")
                .arg("--dev")
                .arg("/dev")
                .arg("--proc")
                .arg("/proc")
                .arg("--tmpfs")
                .arg("/tmp");

            // Bind mounts
            for mount in &vfs.validated_mounts {
                let p = mount.host_path.to_string_lossy().to_string();
                match mount.mount_type {
                    MountType::ReadOnly => {
                        b.arg("--ro-bind").arg(&p).arg(&p);
                    }
                    MountType::ReadWrite | MountType::Scratchpad => {
                        b.arg("--bind").arg(&p).arg(&p);
                    }
                }
            }

            b.arg("--").arg(&config.executable);
            b
        } else {
            // Direct command execution with virtual environment overrides
            Command::new(&config.executable)
        };

        cmd.args(&config.args);

        if let Some(ref wd) = config.working_dir {
            cmd.current_dir(wd);
        } else if !config.stage_copies.is_empty() {
            cmd.current_dir(vfs.scratchpad.workspace());
        } else if let Some(first_mount) = config.mounts.first() {
            cmd.current_dir(&first_mount.host_path);
        } else {
            cmd.current_dir(vfs.scratchpad.workspace());
        }

        // Apply environment
        for (k, v) in safe_env {
            cmd.env(k, v);
        }
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

        let mut child = cmd.spawn().map_err(CocoonError::Io)?;

        if let Some(ref data) = config.stdin_data {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(data);
            }
        }

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
        }

        if let Some(mut err) = stderr_handle.take() {
            thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = err.read_to_end(&mut buf);
                let _ = stderr_tx.send(buf);
            });
        }

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
