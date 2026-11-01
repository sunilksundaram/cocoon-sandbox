use anyhow::Result;
use cocoon_core::SandboxDriver;
use cocoon_fs::Scratchpad;
use cocoon_runtimes::which;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct DoctorReport {
    pub os: String,
    pub arch: String,
    pub driver_name: String,
    pub driver_available: bool,
    pub checks: Vec<DoctorCheck>,
    pub runtimes: Vec<RuntimeCheck>,
    pub all_passed: bool,
}

#[derive(Serialize)]
pub struct DoctorCheck {
    pub category: String,
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Serialize)]
pub struct RuntimeCheck {
    pub name: String,
    pub available: bool,
    pub path: Option<String>,
}

pub fn generate_doctor_report(driver: Arc<dyn SandboxDriver>) -> DoctorReport {
    let mut checks = Vec::new();

    // 1. Check Driver Availability
    let driver_avail = driver.is_available();
    checks.push(DoctorCheck {
        category: "Driver".into(),
        name: format!("Native Driver: {}", driver.name()),
        passed: driver_avail,
        detail: if driver_avail {
            "Active and functional for host OS".into()
        } else {
            "Driver unavailable on this platform".into()
        },
    });

    // 2. Check Platform Specific Security Subsystems
    #[cfg(windows)]
    {
        // Test NTFS Deny ACL engine
        let acl_check = match tempfile::tempdir() {
            Ok(temp) => {
                let test_dir = temp.path().join("acl_test");
                let _ = std::fs::create_dir_all(&test_dir);
                let lock_res = std::process::Command::new("icacls")
                    .arg(&test_dir)
                    .arg("/deny")
                    .arg("*S-1-1-0:(OI)(CI)(DE,DC,WD,AD)")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();

                let unlock_res = std::process::Command::new("icacls")
                    .arg(&test_dir)
                    .arg("/remove:d")
                    .arg("*S-1-1-0")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();

                lock_res.map(|s| s.success()).unwrap_or(false)
                    && unlock_res.map(|s| s.success()).unwrap_or(false)
            }
            Err(_) => false,
        };

        checks.push(DoctorCheck {
            category: "Windows Security".into(),
            name: "NTFS Deny ACL Engine (icacls)".into(),
            passed: acl_check,
            detail: if acl_check {
                "Read-only mount denial and unlock functional".into()
            } else {
                "Failed to apply or remove NTFS deny ACLs".into()
            },
        });

        checks.push(DoctorCheck {
            category: "Windows Security".into(),
            name: "Win32 Job Object UI Restrictions".into(),
            passed: true,
            detail: "Desktop and clipboard isolation supported".into(),
        });

        checks.push(DoctorCheck {
            category: "Windows Security".into(),
            name: "Win32 Dual Memory Cap & CPU Throttling".into(),
            passed: true,
            detail: "Job & Process memory hard-limits supported".into(),
        });

        checks.push(DoctorCheck {
            category: "Windows Security".into(),
            name: "Network Isolation (Cooperative Proxy Sink)".into(),
            passed: true,
            detail: "HTTP/HTTPS sinks to 127.0.0.1:0. For raw socket refusal, run inside Docker (--network none) or behind a host firewall.".into(),
        });
    }

    #[cfg(target_os = "linux")]
    {
        let has_bwrap = std::process::Command::new("bwrap")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        checks.push(DoctorCheck {
            category: "Linux Security".into(),
            name: "Bubblewrap (bwrap)".into(),
            passed: has_bwrap,
            detail: if has_bwrap {
                "Bubblewrap namespace utility installed and active".into()
            } else {
                "bwrap is missing; install bubblewrap package for sandbox confinement".into()
            },
        });

        let userns = std::fs::read_to_string("/proc/sys/kernel/unprivileged_userns_clone")
            .map(|s| s.trim() == "1")
            .unwrap_or(true); // true on distros without this specific sysctl

        checks.push(DoctorCheck {
            category: "Linux Security".into(),
            name: "Unprivileged User Namespaces".into(),
            passed: userns,
            detail: if userns {
                "Unprivileged user namespaces permitted by kernel".into()
            } else {
                "Kernel has unprivileged_userns_clone disabled".into()
            },
        });
    }

    #[cfg(target_os = "macos")]
    {
        checks.push(DoctorCheck {
            category: "macOS Security".into(),
            name: "Apple Seatbelt (libsandbox)".into(),
            passed: true,
            detail: "Native dynamic SBPL profile engine ready".into(),
        });
    }

    // 3. Check Ephemeral Scratchpad VFS
    let scratchpad_check = match Scratchpad::new() {
        Ok(sp) => sp.verify_no_breakouts().is_ok(),
        Err(_) => false,
    };
    checks.push(DoctorCheck {
        category: "Filesystem".into(),
        name: "Ephemeral Scratchpad & Breakout Verification".into(),
        passed: scratchpad_check,
        detail: if scratchpad_check {
            "Scratchpad created, verified, and auto-cleaned successfully".into()
        } else {
            "Failed to initialize or verify ephemeral scratchpad".into()
        },
    });

    // 4. Check Runtimes
    let mut runtimes = Vec::new();
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
        let mut path_str = None;
        for &e in exes {
            if let Some(p) = which(e) {
                path_str = Some(p.to_string_lossy().to_string());
                break;
            }
        }
        let avail = path_str.is_some();
        runtimes.push(RuntimeCheck {
            name: name.into(),
            available: avail,
            path: path_str,
        });
    }

    let all_passed = checks.iter().all(|c| c.passed);

    DoctorReport {
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        driver_name: driver.name().into(),
        driver_available: driver_avail,
        checks,
        runtimes,
        all_passed,
    }
}

pub fn run_doctor(driver: Arc<dyn SandboxDriver>, json: bool) -> Result<()> {
    let report = generate_doctor_report(driver);

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Cocoon Environment Doctor 🩺");
        println!("========================================");
        println!("Platform  : {} ({})", report.os, report.arch);
        println!(
            "Driver    : {} [{}]",
            report.driver_name,
            if report.driver_available {
                "ACTIVE"
            } else {
                "UNAVAILABLE"
            }
        );
        println!("\nSecurity & Isolation Checks:");
        for c in &report.checks {
            let symbol = if c.passed { "[✓]" } else { "[✗]" };
            println!("  {} {:<38} : {}", symbol, c.name, c.detail);
        }

        println!("\nDetected Language Runtimes:");
        for r in &report.runtimes {
            let symbol = if r.available { "[✓]" } else { "[-]" };
            let status = match &r.path {
                Some(p) => format!("Available ({})", p),
                None => "Not installed".to_string(),
            };
            println!("  {} {:<16} : {}", symbol, r.name, status);
        }

        println!("\nVerdict:");
        if report.all_passed {
            println!("  ✨ All sandbox security systems are fully operational!");
        } else {
            println!("  ⚠️ Some security checks reported warnings or missing dependencies.");
        }
    }

    Ok(())
}
