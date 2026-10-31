use cocoon_core::{
    CocoonError, ExecutionResult, MountType, SandboxConfig, SandboxDriver, TerminationReason,
};
use cocoon_fs::VirtualView;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tracing::warn;

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

        // Enforce fail-closed policy: Bubblewrap is required for namespace isolation
        if !self.has_bwrap() {
            return Err(CocoonError::SecurityViolation(
                "Bubblewrap (bwrap) is required for sandbox isolation on Linux. Falling back to an unconfined host process is forbidden.".into(),
            ));
        }

        let mut b = Command::new("bwrap");
        b.arg("--unshare-all")
            .arg("--die-with-parent")
            .arg("--dev")
            .arg("/dev")
            .arg("--proc")
            .arg("/proc")
            .arg("--tmpfs")
            .arg("/tmp")
            .arg("--tmpfs")
            .arg("/dev/shm");

        // Network isolation
        if config.limits.allow_network {
            b.arg("--share-net");
        }

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
        let mut cmd = b;

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

        // Enforce GPU isolation or safety guards
        if !config.limits.allow_gpu {
            cmd.env("CUDA_VISIBLE_DEVICES", "");
            cmd.env("ROCR_VISIBLE_DEVICES", "");
            cmd.env("HIP_VISIBLE_DEVICES", "");
            cmd.env("OPENCL_DEVICE_NAME", "");
            cmd.env("GPU_DEVICE_ORDINAL", "");
            cmd.env("NVIDIA_VISIBLE_DEVICES", "void");
            cmd.env("ONEAPI_DEVICE_SELECTOR", "none");
            cmd.env("SYCL_DEVICE_FILTER", "none");
        } else {
            cmd.env("PYTORCH_CUDA_ALLOC_CONF", "max_split_size_mb:128");
        }

        // Explicitly strip host credential and IPC socket environment variables even if inherit_env is enabled
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

        cmd.stdin(if config.stdin_data.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Apply Linux resource caps (memory limit, process limit, priority, and file size quota)
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            let mem_mb = config.limits.memory_limit_mb;
            let max_p = config.limits.max_processes;
            let disk_mb = config.limits.disk_limit_mb;
            unsafe {
                cmd.pre_exec(move || {
                    // Auto-detect host physical memory to enforce strict 95% ceiling
                    let host_phys_mb = {
                        let pages = libc::sysconf(libc::_SC_PHYS_PAGES);
                        let page_size = libc::sysconf(libc::_SC_PAGESIZE);
                        if pages > 0 && page_size > 0 {
                            ((pages as u64 * page_size as u64) / (1024 * 1024)) as usize
                        } else {
                            4096
                        }
                    };
                    let max_safe_mem_mb = (host_phys_mb * 95) / 100;
                    let effective_mem_mb = mem_mb.unwrap_or(max_safe_mem_mb).min(max_safe_mem_mb);
                    let bytes = (effective_mem_mb as u64) * 1024 * 1024;
                    let rlim_mem = libc::rlimit {
                        rlim_cur: bytes,
                        rlim_max: bytes,
                    };
                    libc::setrlimit(libc::RLIMIT_AS, &rlim_mem);

                    // Block real-time priority elevation to prevent preempting host OS scheduler
                    let rlim_rtprio = libc::rlimit {
                        rlim_cur: 0,
                        rlim_max: 0,
                    };
                    libc::setrlimit(libc::RLIMIT_RTPRIO, &rlim_rtprio);

                    // Suppress core dumps to avoid disk exhaustion and credential leaks
                    let rlim_core = libc::rlimit {
                        rlim_cur: 0,
                        rlim_max: 0,
                    };
                    libc::setrlimit(libc::RLIMIT_CORE, &rlim_core);

                    if let Some(procs) = max_p {
                        let rlim = libc::rlimit {
                            rlim_cur: procs as u64,
                            rlim_max: procs as u64,
                        };
                        libc::setrlimit(libc::RLIMIT_NPROC, &rlim);
                    }
                    if let Some(mb) = disk_mb {
                        let bytes = (mb * 1024 * 1024) as u64;
                        let rlim = libc::rlimit {
                            rlim_cur: bytes,
                            rlim_max: bytes,
                        };
                        libc::setrlimit(libc::RLIMIT_FSIZE, &rlim);
                    }
                    Ok(())
                });
            }
        }

        let mut child = cmd.spawn().map_err(CocoonError::Io)?;

        if let Some(ref data) = config.stdin_data {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(data);
            }
        }

        let mut stdout_handle = child.stdout.take();
        let mut stderr_handle = child.stderr.take();

        let max_output = config.limits.max_output_bytes.unwrap_or(10 * 1024 * 1024);
        let (stdout_tx, stdout_rx) = mpsc::channel();
        let (stderr_tx, stderr_rx) = mpsc::channel();

        if let Some(mut out) = stdout_handle.take() {
            thread::spawn(move || {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                let mut truncated = false;
                loop {
                    match out.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
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

        let timeout = config.limits.timeout;
        let mut timed_out = false;
        #[allow(unused_mut)]
        let mut oom_killed = false;
        let mut termination_reason = None;
        let mut exit_code = -1;

        if let Some(duration) = timeout {
            let poll_interval = Duration::from_millis(10);
            let deadline = Instant::now() + duration;

            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        exit_code = status.code().unwrap_or(-1);
                        #[cfg(unix)]
                        {
                            use std::os::unix::process::ExitStatusExt;
                            if status.signal() == Some(9) || exit_code == 137 {
                                if config.limits.memory_limit_mb.is_some() {
                                    oom_killed = true;
                                    termination_reason = Some(TerminationReason::MemoryExceeded);
                                }
                            }
                        }
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
                                "Process exceeded execution timeout limit on Linux; terminating child"
                            );
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
                Ok(status) => {
                    exit_code = status.code().unwrap_or(-1);
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        if status.signal() == Some(9) || exit_code == 137 {
                            if config.limits.memory_limit_mb.is_some() {
                                oom_killed = true;
                                termination_reason = Some(TerminationReason::MemoryExceeded);
                            }
                        }
                    }
                }
                Err(e) => return Err(CocoonError::Io(e)),
            }
        }

        let (stdout_bytes, stdout_truncated) = stdout_rx.recv().unwrap_or_default();
        let (stderr_bytes, stderr_truncated) = stderr_rx.recv().unwrap_or_default();
        let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;

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
            peak_memory_mb: None,
        })
    }
}
