use cocoon_core::{CocoonError, ExecutionResult, MountType, SandboxConfig, SandboxDriver};
use cocoon_fs::VirtualView;

#[cfg(test)]
use cocoon_core::MountRule;

#[cfg(target_os = "macos")]
use cocoon_core::TerminationReason;
#[cfg(target_os = "macos")]
use std::ffi::CString;
#[cfg(target_os = "macos")]
use std::io::{Read, Write};
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
#[cfg(target_os = "macos")]
use std::sync::mpsc;
#[cfg(target_os = "macos")]
use std::thread;
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
extern "C" {
    fn sandbox_init(
        profile: *const std::ffi::c_char,
        flags: u64,
        errorbuf: *mut *mut std::ffi::c_char,
    ) -> std::ffi::c_int;
    fn sandbox_free_error(errorbuf: *mut std::ffi::c_char);
}

/// macOS Sandbox Driver using Apple Seatbelt (libsandbox / sandbox_init)
#[derive(Default)]
pub struct MacOsSeatbeltDriver;

impl MacOsSeatbeltDriver {
    pub fn new() -> Self {
        Self
    }

    /// Generates dynamic SBPL (Sandbox Profile Language) scheme profile
    pub fn generate_sbpl_profile(config: &SandboxConfig, vfs: &VirtualView) -> String {
        let mut profile = String::from(
            "(version 1)\n(deny default)\n(allow process-exec)\n(allow process-fork)\n(allow sysctl-read)\n(allow signal (target self))\n(allow file-read-metadata)\n",
        );

        // Allow reading essential system libraries, framework bundles, and system binaries
        profile.push_str("(allow file-read*\n");
        profile.push_str("  (subpath \"/System\")\n");
        profile.push_str("  (subpath \"/Library\")\n");
        profile.push_str("  (subpath \"/usr/lib\")\n");
        profile.push_str("  (subpath \"/usr/share\")\n");
        profile.push_str("  (subpath \"/usr/bin\")\n");
        profile.push_str("  (subpath \"/bin\")\n");
        profile.push_str("  (subpath \"/sbin\")\n");
        profile.push_str("  (subpath \"/opt/homebrew\")\n");
        profile.push_str("  (subpath \"/usr/local\")\n");
        profile.push_str("  (literal \"/dev/null\")\n");
        profile.push_str("  (literal \"/dev/zero\")\n");
        profile.push_str("  (literal \"/dev/urandom\")\n");
        profile.push_str("  (literal \"/dev/random\")\n");
        profile.push_str("  (literal \"/dev/dtracehelper\")\n");

        // Read-only mounts
        for mount in &vfs.validated_mounts {
            if mount.mount_type == MountType::ReadOnly {
                profile.push_str(&format!(
                    "  (subpath \"{}\")\n",
                    mount.host_path.to_string_lossy().replace('\\', "/")
                ));
            }
        }
        profile.push_str(")\n");

        // Allow writing to /dev/null and /dev/zero
        profile.push_str("(allow file-write-data\n");
        profile.push_str("  (literal \"/dev/null\")\n");
        profile.push_str("  (literal \"/dev/zero\")\n");
        profile.push_str(")\n");

        // Writable workspace and scratchpad
        profile.push_str("(allow file*\n");
        profile.push_str(&format!(
            "  (subpath \"{}\")\n",
            vfs.scratchpad.root().to_string_lossy().replace('\\', "/")
        ));
        profile.push_str(&format!(
            "  (subpath \"{}\")\n",
            vfs.scratchpad
                .workspace()
                .to_string_lossy()
                .replace('\\', "/")
        ));
        for mount in &vfs.validated_mounts {
            if mount.mount_type != MountType::ReadOnly {
                profile.push_str(&format!(
                    "  (subpath \"{}\")\n",
                    mount.host_path.to_string_lossy().replace('\\', "/")
                ));
            }
        }
        profile.push_str(")\n");

        // Network isolation
        if config.limits.allow_network {
            profile.push_str("(allow network*)\n");
        }

        profile
    }
}

impl SandboxDriver for MacOsSeatbeltDriver {
    fn name(&self) -> &'static str {
        "macos-seatbelt"
    }

    fn is_available(&self) -> bool {
        cfg!(target_os = "macos")
    }

    fn execute(&self, config: &SandboxConfig) -> Result<ExecutionResult, CocoonError> {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = config;
            Err(CocoonError::Platform(
                "MacOsSeatbeltDriver is only supported on macOS".into(),
            ))
        }

        #[cfg(target_os = "macos")]
        {
            let start_time = Instant::now();
            let vfs = VirtualView::setup(
                &config.mounts,
                config.temp_dir.as_deref(),
                config.working_dir.as_deref(),
                &config.stage_copies,
            )?;
            let safe_env = vfs.safe_env_redirects();

            let sbpl = Self::generate_sbpl_profile(config, &vfs);
            let sbpl_c = CString::new(sbpl).map_err(|e| CocoonError::Platform(e.to_string()))?;

            let mut cmd = Command::new(&config.executable);
            cmd.args(&config.args);

            if let Some(ref wd) = config.working_dir {
                cmd.current_dir(wd);
            } else if !config.stage_copies.is_empty() {
                cmd.current_dir(vfs.scratchpad.workspace());
            } else if let Some(first_mount) = config.mounts.first().filter(|m| m.host_path.is_dir())
            {
                cmd.current_dir(&first_mount.host_path);
            } else {
                cmd.current_dir(vfs.scratchpad.workspace());
            }

            cmd.envs(&safe_env);
            if !config.inherit_env {
                cmd.env_remove("AWS_ACCESS_KEY_ID");
                cmd.env_remove("AWS_SECRET_ACCESS_KEY");
                cmd.env_remove("GITHUB_TOKEN");
                cmd.env_remove("OPENAI_API_KEY");
                cmd.env_remove("ANTHROPIC_API_KEY");
                cmd.env_remove("GEMINI_API_KEY");
            }
            cmd.envs(&config.env);

            let mem_limit_mb = config.limits.memory_limit_mb;
            let max_procs = config.limits.max_processes;

            unsafe {
                use std::os::unix::process::CommandExt;
                cmd.pre_exec(move || {
                    let mut err_buf = std::ptr::null_mut();
                    let ret = sandbox_init(sbpl_c.as_ptr(), 0, &mut err_buf);
                    if ret != 0 {
                        if !err_buf.is_null() {
                            sandbox_free_error(err_buf);
                        }
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            "sandbox_init failed",
                        ));
                    }

                    if let Some(limit_mb) = mem_limit_mb {
                        let bytes = (limit_mb as libc::rlim_t) * 1024 * 1024;
                        let rlim_mem = libc::rlimit {
                            rlim_cur: bytes,
                            rlim_max: bytes,
                        };
                        let _ = libc::setrlimit(libc::RLIMIT_AS, &rlim_mem);
                    }

                    if let Some(procs) = max_procs {
                        let rlim_procs = libc::rlimit {
                            rlim_cur: procs as libc::rlim_t,
                            rlim_max: procs as libc::rlim_t,
                        };
                        let _ = libc::setrlimit(libc::RLIMIT_NPROC, &rlim_procs);
                    }

                    let rlim_core = libc::rlimit {
                        rlim_cur: 0,
                        rlim_max: 0,
                    };
                    let _ = libc::setrlimit(libc::RLIMIT_CORE, &rlim_core);

                    Ok(())
                });
            }

            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            cmd.stdin(if config.stdin_data.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });

            let mut child = cmd.spawn().map_err(CocoonError::Io)?;

            if let Some(ref input) = config.stdin_data {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(input);
                }
            }

            let max_bytes = config.limits.max_output_bytes.unwrap_or(10 * 1024 * 1024);
            let mut stdout_handle = child.stdout.take();
            let mut stderr_handle = child.stderr.take();

            let (tx_out, rx_out) = mpsc::channel();
            let (tx_err, rx_err) = mpsc::channel();

            let stream_tx_out = config.stream_sender.clone();
            let stream_tx_err = config.stream_sender.clone();

            thread::spawn(move || {
                let mut buffer = Vec::new();
                let mut truncated = false;
                if let Some(ref mut out) = stdout_handle {
                    let mut chunk = [0u8; 8192];
                    while let Ok(n) = out.read(&mut chunk) {
                        if n == 0 {
                            break;
                        }
                        if let Some(ref stx) = stream_tx_out {
                            let _ = stx.send(cocoon_core::StreamChunk::Stdout(
                                String::from_utf8_lossy(&chunk[..n]).to_string(),
                            ));
                        }
                        if buffer.len() + n <= max_bytes {
                            buffer.extend_from_slice(&chunk[..n]);
                        } else {
                            let remain = max_bytes.saturating_sub(buffer.len());
                            buffer.extend_from_slice(&chunk[..remain]);
                            truncated = true;
                            break;
                        }
                    }
                }
                let _ = tx_out.send((buffer, truncated));
            });

            thread::spawn(move || {
                let mut buffer = Vec::new();
                let mut truncated = false;
                if let Some(ref mut err) = stderr_handle {
                    let mut chunk = [0u8; 8192];
                    while let Ok(n) = err.read(&mut chunk) {
                        if n == 0 {
                            break;
                        }
                        if let Some(ref stx) = stream_tx_err {
                            let _ = stx.send(cocoon_core::StreamChunk::Stderr(
                                String::from_utf8_lossy(&chunk[..n]).to_string(),
                            ));
                        }
                        if buffer.len() + n <= max_bytes {
                            buffer.extend_from_slice(&chunk[..n]);
                        } else {
                            let remain = max_bytes.saturating_sub(buffer.len());
                            buffer.extend_from_slice(&chunk[..remain]);
                            truncated = true;
                            break;
                        }
                    }
                }
                let _ = tx_err.send((buffer, truncated));
            });

            let timeout = config.limits.timeout.unwrap_or(Duration::from_secs(30));
            let check_interval = Duration::from_millis(10);
            let mut timed_out = false;
            let mut exit_status = None;

            loop {
                match child.try_wait().map_err(CocoonError::Io)? {
                    Some(s) => {
                        exit_status = Some(s);
                        break;
                    }
                    None => {
                        if start_time.elapsed() >= timeout {
                            timed_out = true;
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                        thread::sleep(check_interval);
                    }
                }
            }

            let duration_ms = start_time.elapsed().as_secs_f64() * 1000.0;
            let (out_bytes, stdout_truncated) = rx_out
                .recv_timeout(Duration::from_millis(500))
                .unwrap_or_default();
            let (err_bytes, stderr_truncated) = rx_err
                .recv_timeout(Duration::from_millis(500))
                .unwrap_or_default();

            vfs.scratchpad.verify_no_breakouts()?;

            let exit_code = if timed_out {
                124
            } else if let Some(ref s) = exit_status {
                s.code().unwrap_or(1)
            } else {
                1
            };
            let oom_killed = exit_code == 137;

            let termination_reason = if timed_out {
                Some(TerminationReason::Timeout)
            } else if oom_killed {
                Some(TerminationReason::MemoryExceeded)
            } else {
                None
            };

            Ok(ExecutionResult {
                exit_code,
                stdout: String::from_utf8_lossy(&out_bytes).to_string(),
                stderr: String::from_utf8_lossy(&err_bytes).to_string(),
                duration_ms,
                timed_out,
                oom_killed,
                peak_memory_mb: None,
                stdout_truncated,
                stderr_truncated,
                termination_reason,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_generate_sbpl_profile_offline() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ro_dir = temp.path().join("ro_dir");
        std::fs::create_dir_all(&ro_dir).expect("create dir");

        let mounts = vec![MountRule {
            host_path: ro_dir.clone(),
            guest_path: None,
            mount_type: MountType::ReadOnly,
        }];
        let vfs = VirtualView::setup(&mounts, None, None, &[]).expect("VirtualView setup");
        let config = SandboxConfig {
            executable: std::path::PathBuf::from("/bin/echo"),
            args: vec!["hello".into()],
            working_dir: None,
            temp_dir: None,
            stage_copies: vec![],
            env: Default::default(),
            inherit_env: false,
            mounts,
            limits: cocoon_core::ResourceLimits {
                allow_network: false,
                ..Default::default()
            },
            stdin_data: None,
            stream_sender: None,
        };

        let profile = MacOsSeatbeltDriver::generate_sbpl_profile(&config, &vfs);
        assert!(profile.contains("(deny default)"));
        assert!(!profile.contains("(allow network*)"));
        let mounted_path = vfs.validated_mounts[0]
            .host_path
            .to_string_lossy()
            .replace('\\', "/");
        assert!(profile.contains(&mounted_path));
    }

    #[test]
    fn test_generate_sbpl_profile_network_allowed() {
        let mounts = vec![];
        let vfs = VirtualView::setup(&mounts, None, None, &[]).expect("VirtualView setup");
        let config = SandboxConfig {
            executable: PathBuf::from("/bin/echo"),
            args: vec!["hello".into()],
            working_dir: None,
            temp_dir: None,
            stage_copies: vec![],
            env: Default::default(),
            inherit_env: false,
            mounts,
            limits: cocoon_core::ResourceLimits {
                allow_network: true,
                ..Default::default()
            },
            stdin_data: None,
            stream_sender: None,
        };

        let profile = MacOsSeatbeltDriver::generate_sbpl_profile(&config, &vfs);
        assert!(profile.contains("(allow network*)"));
    }
}
