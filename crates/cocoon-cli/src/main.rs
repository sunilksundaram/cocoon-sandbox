use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use cocoon_core::{MountRule, MountType, ResourceLimits, SandboxConfig};
use cocoon_fs::VirtualView;
use cocoon_platform::get_default_driver;
use cocoon_runtimes::{which, RuntimeKind, RuntimePreset};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(
    name = "cocoon",
    version,
    about = "Featherweight cross-platform sandbox with virtual and read-only filesystems for AI agents and code execution",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Execute a script, command, or runtime inside the sandbox
    Run {
        /// Mount host path as strictly Read-Only (can be repeated, default: current directory '.')
        #[arg(long = "ro", value_name = "PATH")]
        read_only: Vec<PathBuf>,

        /// Mount host path as Read-Write (can be repeated)
        #[arg(long = "rw", value_name = "PATH")]
        read_write: Vec<PathBuf>,

        /// Runtime preset to execute (python, node, bun, deno, pwsh, bash, rust, go, java, ruby, php, lua)
        #[arg(short = 'r', long = "runtime", value_name = "RUNTIME")]
        runtime: Option<String>,

        /// Inline code or script snippet to execute
        #[arg(short = 'c', long = "code", value_name = "STRING")]
        code: Option<String>,

        /// Path to a script file to execute
        #[arg(short = 'f', long = "file", value_name = "FILE")]
        file: Option<PathBuf>,

        /// Working directory inside the sandbox where commands execute and files can be created/modified
        #[arg(
            short = 'w',
            long = "work",
            visible_alias = "workdir",
            value_name = "DIR"
        )]
        workdir: Option<PathBuf>,

        /// Custom temporary directory for process temp files and caches (sets TMP/TEMP/TMPDIR)
        #[arg(short = 't', long = "temp", visible_alias = "tmp", value_name = "DIR")]
        tempdir: Option<PathBuf>,

        /// Copy a file or directory into the work folder before execution (can be repeated)
        #[arg(long = "copy", visible_alias = "stage", value_name = "PATH")]
        copy: Vec<PathBuf>,

        /// Execution timeout in seconds (default: 30s)
        #[arg(long = "timeout", value_name = "SECS")]
        timeout: Option<u64>,

        /// Maximum memory limit in Megabytes
        #[arg(long = "mem", value_name = "MB")]
        memory_mb: Option<usize>,

        /// Format execution summary as JSON (ideal for AI Agents)
        #[arg(long = "json")]
        json: bool,

        /// Explicit command and trailing arguments (if not using --runtime)
        #[arg(last = true)]
        raw_cmd: Vec<String>,
    },

    /// Execute a multi-language polyglot pipeline in the exact same virtual filesystem
    Pipeline {
        /// Mount host path as strictly Read-Only (can be repeated)
        #[arg(long = "ro", value_name = "PATH")]
        read_only: Vec<PathBuf>,

        /// Working directory inside the sandbox (default: ephemeral shared workspace)
        #[arg(
            short = 'w',
            long = "work",
            visible_alias = "workdir",
            value_name = "DIR"
        )]
        workdir: Option<PathBuf>,

        /// Custom temporary directory (sets TMP/TEMP/TMPDIR)
        #[arg(short = 't', long = "temp", visible_alias = "tmp", value_name = "DIR")]
        tempdir: Option<PathBuf>,

        /// Copy a file or directory into the work folder before execution (can be repeated)
        #[arg(long = "copy", visible_alias = "stage", value_name = "PATH")]
        copy: Vec<PathBuf>,

        /// Sequential execution step formatted as "<runtime>:<code or file>", e.g. "python:import os; ..."
        #[arg(short = 's', long = "step", value_name = "RUNTIME:CODE_OR_FILE")]
        steps: Vec<String>,

        /// Execution timeout in seconds per step
        #[arg(long = "timeout", value_name = "SECS")]
        timeout: Option<u64>,

        /// Format execution summary as JSON (ideal for AI Agents)
        #[arg(long = "json")]
        json: bool,
    },

    /// Display host sandbox capabilities, driver status, and detected runtimes
    Info,
}

#[derive(Serialize)]
struct PipelineStepReport {
    step: usize,
    runtime: String,
    exit_code: i32,
    stdout: String,
    stderr: String,
    duration_ms: f64,
    timed_out: bool,
}

#[derive(Serialize)]
struct PipelineReport {
    success: bool,
    total_duration_ms: f64,
    steps: Vec<PipelineStepReport>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();
    let driver = get_default_driver();

    match cli.command {
        Commands::Info => {
            println!("Cocoon Sandbox v{}", env!("CARGO_PKG_VERSION"));
            println!("----------------------------------------");
            println!("Active Driver : {}", driver.name());
            println!(
                "Driver Status : {}",
                if driver.is_available() {
                    "Available (Active)"
                } else {
                    "Unavailable"
                }
            );
            println!("Target OS     : {}", std::env::consts::OS);
            println!("Target Arch   : {}", std::env::consts::ARCH);
            println!("\nDetected System Runtimes:");
            for (name, exes) in [
                ("Python", &["python3", "python"][..]),
                ("Node.js", &["node", "nodejs"][..]),
                ("Bun", &["bun"][..]),
                ("Deno", &["deno"][..]),
                ("Go", &["go"][..]),
                ("Java", &["java"][..]),
                ("Rust/Cargo", &["cargo"][..]),
                ("PowerShell", &["pwsh", "powershell.exe"][..]),
                ("Bash/Shell", &["bash", "sh"][..]),
                ("Ruby", &["ruby"][..]),
                ("PHP", &["php"][..]),
                ("Lua", &["lua", "luajit"][..]),
            ] {
                let found = exes.iter().any(|&e| which(e).is_some());
                println!(
                    "  - {:<12} : {}",
                    name,
                    if found { "✓ Ready" } else { "✗ Not found" }
                );
            }
            Ok(())
        }

        Commands::Run {
            read_only,
            read_write,
            runtime,
            code,
            file,
            workdir,
            tempdir,
            copy,
            timeout,
            memory_mb,
            json,
            raw_cmd,
        } => {
            let mut mounts = Vec::new();

            // Default: LLMs/agents can never get write access to anything other than the work folder.
            // If no --ro is specified, mount the current directory as ReadOnly by default!
            let effective_ro = if read_only.is_empty() {
                vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
            } else {
                read_only
            };

            for ro in effective_ro {
                mounts.push(MountRule {
                    host_path: ro,
                    guest_path: None,
                    mount_type: MountType::ReadOnly,
                });
            }

            for rw in read_write {
                mounts.push(MountRule {
                    host_path: rw,
                    guest_path: None,
                    mount_type: MountType::ReadWrite,
                });
            }

            let (executable, args) = if let Some(rt_str) = runtime {
                let kind: RuntimeKind = rt_str.parse().map_err(|e| anyhow::anyhow!("{}", e))?;
                RuntimePreset::configure(kind, code.as_deref(), file.as_deref())
                    .context("Failed to configure runtime preset")?
            } else if !raw_cmd.is_empty() {
                let exe = PathBuf::from(&raw_cmd[0]);
                let rest = raw_cmd[1..].to_vec();
                (exe, rest)
            } else {
                anyhow::bail!("Specify either a --runtime (-r) or an explicit command after '--'");
            };

            // Default: 30 seconds timeout if not explicitly provided
            let effective_timeout = Duration::from_secs(timeout.unwrap_or(30));

            let limits = ResourceLimits {
                timeout: Some(effective_timeout),
                memory_limit_mb: memory_mb,
                cpu_limit_pct: None,
                allow_network: true,
            };

            let config = SandboxConfig {
                executable,
                args,
                working_dir: workdir,
                temp_dir: tempdir,
                stage_copies: copy,
                env: Default::default(),
                inherit_env: false,
                mounts,
                limits,
                stdin_data: None,
            };

            let result = driver
                .execute(&config)
                .context("Sandbox execution failed")?;

            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                if !result.stdout.is_empty() {
                    print!("{}", result.stdout);
                }
                if !result.stderr.is_empty() {
                    eprint!("{}", result.stderr);
                }
                if result.timed_out {
                    eprintln!("\n[cocoon] Process terminated: execution timed out.");
                }
            }

            std::process::exit(result.exit_code);
        }

        Commands::Pipeline {
            read_only,
            workdir,
            tempdir,
            copy,
            steps,
            timeout,
            json,
        } => {
            if steps.is_empty() {
                anyhow::bail!(
                    "No pipeline steps specified. Use --step 'runtime:code' to add steps."
                );
            }

            let mut mounts = Vec::new();

            // Default: LLMs/agents can never get write access to anything other than the work folder.
            // If no --ro is specified, mount the current directory as ReadOnly by default!
            let effective_ro = if read_only.is_empty() {
                vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
            } else {
                read_only
            };

            for ro in effective_ro {
                mounts.push(MountRule {
                    host_path: ro,
                    guest_path: None,
                    mount_type: MountType::ReadOnly,
                });
            }

            // Setup the shared virtual filesystem view ONCE for all steps in the pipeline
            let vfs = VirtualView::setup(&mounts, tempdir.as_deref(), workdir.as_deref(), &copy)?;
            let _ro_guard = vfs.lock_read_only()?;
            let shared_workdir = vfs.scratchpad.workspace().to_path_buf();

            let pipeline_start = Instant::now();
            let mut step_reports = Vec::new();
            let mut overall_success = true;

            let effective_timeout = Duration::from_secs(timeout.unwrap_or(30));

            for (idx, step_str) in steps.iter().enumerate() {
                let (rt_name, content) = match step_str.split_once(':') {
                    Some((r, c)) => (r.trim(), c.trim()),
                    None => anyhow::bail!(
                        "Invalid step format '{}'. Expected '<runtime>:<code or file>'",
                        step_str
                    ),
                };

                let kind: RuntimeKind = rt_name.parse().map_err(|e| anyhow::anyhow!("{}", e))?;

                // Check if content is an existing file
                let path_candidate = Path::new(content);
                let (executable, args) = if path_candidate.is_file() {
                    RuntimePreset::configure(kind, None, Some(path_candidate))?
                } else {
                    RuntimePreset::configure(kind, Some(content), None)?
                };

                let config = SandboxConfig {
                    executable,
                    args,
                    working_dir: Some(shared_workdir.clone()),
                    temp_dir: Some(vfs.scratchpad.root().to_path_buf()),
                    stage_copies: Vec::new(),
                    env: Default::default(),
                    inherit_env: false,
                    mounts: Vec::new(), // Already protected by _ro_guard
                    limits: ResourceLimits {
                        timeout: Some(effective_timeout),
                        memory_limit_mb: None,
                        cpu_limit_pct: None,
                        allow_network: true,
                    },
                    stdin_data: None,
                };

                if !json {
                    println!("[cocoon pipeline] Running step {} ({})", idx + 1, rt_name);
                }

                let result = driver.execute(&config)?;

                if !json {
                    if !result.stdout.is_empty() {
                        print!("{}", result.stdout);
                    }
                    if !result.stderr.is_empty() {
                        eprint!("{}", result.stderr);
                    }
                }

                let step_success = result.success();
                step_reports.push(PipelineStepReport {
                    step: idx + 1,
                    runtime: rt_name.to_string(),
                    exit_code: result.exit_code,
                    stdout: result.stdout,
                    stderr: result.stderr,
                    duration_ms: result.duration_ms,
                    timed_out: result.timed_out,
                });

                if !step_success {
                    overall_success = false;
                    if !json {
                        eprintln!(
                            "[cocoon pipeline] Step {} failed (exit code {}). Aborting pipeline.",
                            idx + 1,
                            result.exit_code
                        );
                    }
                    break;
                }
            }

            let report = PipelineReport {
                success: overall_success,
                total_duration_ms: pipeline_start.elapsed().as_secs_f64() * 1000.0,
                steps: step_reports,
            };

            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            }

            if !overall_success {
                std::process::exit(1);
            }

            Ok(())
        }
    }
}
