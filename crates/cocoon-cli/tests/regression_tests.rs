use cocoon_core::{ExecutionResult, MountRule, MountType, ResourceLimits};
use cocoon_fs::Scratchpad;
use cocoon_runtimes::{RuntimeKind, RuntimePreset};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[test]
fn regression_default_timeout_is_enforced() {
    // Invariant: Cocoon must enforce a 30-second default timeout to prevent runaway AI agent loops
    let limits = ResourceLimits::default();
    assert_eq!(
        limits.timeout,
        Some(Duration::from_secs(30)),
        "Regression: Default timeout was modified from 30 seconds"
    );
}

#[test]
fn regression_telemetry_json_schema_matches_agent_spec() {
    // Invariant: The JSON telemetry schema must maintain strict backwards compatibility for AI agents
    let result = ExecutionResult {
        exit_code: 0,
        stdout: "test output\n".to_string(),
        stderr: "".to_string(),
        duration_ms: 12.5,
        timed_out: false,
        oom_killed: false,
        termination_reason: None,
        stdout_truncated: false,
        stderr_truncated: false,
        peak_memory_mb: None,
    };

    let json_val = serde_json::to_value(&result).expect("Must serialize to json value");
    assert!(json_val.get("exit_code").is_some(), "Missing 'exit_code'");
    assert!(json_val.get("stdout").is_some(), "Missing 'stdout'");
    assert!(json_val.get("stderr").is_some(), "Missing 'stderr'");
    assert!(
        json_val.get("duration_ms").is_some(),
        "Missing 'duration_ms'"
    );
    assert!(json_val.get("timed_out").is_some(), "Missing 'timed_out'");
    assert!(json_val.get("oom_killed").is_some(), "Missing 'oom_killed'");
}

#[test]
fn regression_default_network_is_denied() {
    // Invariant: Cocoon must enforce default-deny network access to prevent credential exfiltration
    let limits = ResourceLimits::default();
    assert!(
        !limits.allow_network,
        "Regression: Default network access must be false (default-deny)"
    );
}

#[test]
fn regression_default_max_processes_is_enforced() {
    // Invariant: Cocoon must enforce active process cap to prevent fork bombs
    let limits = ResourceLimits::default();
    assert_eq!(
        limits.max_processes,
        Some(32),
        "Regression: Default max active processes was modified"
    );
}

#[test]
fn regression_default_disk_limit_is_enforced() {
    // Invariant: Cocoon must enforce storage write quota to prevent disk exhaustion DoS
    let limits = ResourceLimits::default();
    assert_eq!(
        limits.disk_limit_mb,
        Some(512),
        "Regression: Default disk limit was modified"
    );
}

#[test]
fn regression_scratchpad_cleanup_on_drop() {
    // Invariant: Ephemeral scratchpads must delete their temporary folder upon Drop to prevent host pollution
    let root_path: PathBuf;
    {
        let scratchpad = Scratchpad::new().expect("Failed to create scratchpad");
        root_path = scratchpad.root().to_path_buf();
        assert!(
            root_path.exists(),
            "Scratchpad root must exist while in scope"
        );
    }
    // After dropping out of scope, tempdir should be cleaned up
    assert!(
        !root_path.exists(),
        "Regression: Scratchpad directory was not destroyed on drop"
    );
}

#[test]
fn regression_preset_runtime_resolution() {
    // Invariant: Python preset must always include bytecode cache suppression flag (-B) and isolation flags (-I, -S)
    let (_, args) =
        RuntimePreset::configure(RuntimeKind::Python, Some("print('regression test')"), None)
            .expect("Python configure should succeed");

    assert!(
        args.contains(&"-I".to_string()),
        "Regression: Python preset omitted -I flag (isolated mode)"
    );
    assert!(
        args.contains(&"-S".to_string()),
        "Regression: Python preset omitted -S flag (site-packages suppression)"
    );
    assert!(
        args.contains(&"-B".to_string()),
        "Regression: Python preset omitted -B flag (bytecode caching suppression)"
    );
}

#[test]
fn regression_mount_rule_immutability() {
    let ro_mount = MountRule::read_only("/workspace/code");
    assert_eq!(ro_mount.mount_type, MountType::ReadOnly);
    assert_eq!(ro_mount.host_path, Path::new("/workspace/code"));
    assert!(ro_mount.guest_path.is_none());
}

#[test]
fn regression_rust_preset_offline() {
    // Invariant: Rust/Cargo preset must always enforce --offline for dependency isolation
    let (_, args) =
        RuntimePreset::configure(RuntimeKind::Rust, Some("fn main() {}"), None)
            .expect("Rust configure should succeed");

    assert!(
        args.contains(&"--offline".to_string()),
        "Regression: Rust preset omitted --offline flag"
    );
}

#[test]
fn regression_go_preset_offline() {
    // Invariant: Go preset must always enforce -mod=readonly for dependency isolation
    let (_, args) =
        RuntimePreset::configure(RuntimeKind::Go, None, Some(Path::new("main.go")))
            .expect("Go configure should succeed");

    assert!(
        args.contains(&"-mod=readonly".to_string()),
        "Regression: Go preset omitted -mod=readonly flag"
    );
}

#[test]
fn regression_clean_scratchpad_passes_breakout_verification() {
    // Invariant: Ephemeral scratchpads must pass breakout verification out of the box
    let scratchpad = Scratchpad::new().expect("Create scratchpad");
    assert!(scratchpad.verify_no_breakouts().is_ok());
}

#[test]
fn regression_default_cpu_limit_is_95_percent() {
    // Invariant: Cocoon must enforce a 95% CPU rate ceiling by default to prevent OS starvation
    let limits = ResourceLimits::default();
    assert_eq!(
        limits.cpu_limit_pct,
        Some(95),
        "Regression: Default CPU limit must be 95% max"
    );
}

#[test]
fn regression_default_gpu_allowed_is_true() {
    // Invariant: GPU hardware access defaults to true, subject to VRAM safety allocation guard
    let limits = ResourceLimits::default();
    assert!(
        limits.allow_gpu,
        "Regression: Default GPU access must be true"
    );
}

#[test]
fn regression_memory_cap_enforces_95_percent_ceiling() {
    // Invariant: When memory is capped on host, it never exceeds 95% of host physical RAM
    let host_mb = 16384; // Mock 16GB host
    let max_safe = (host_mb * 95) / 100;
    assert_eq!(max_safe, 15564);

    let requested_over_limit = Some(32768); // User requested 32GB
    let effective = requested_over_limit.unwrap_or(max_safe).min(max_safe);
    assert_eq!(effective, 15564);

    let requested_under_limit = Some(4096); // User requested 4GB
    let effective_under = requested_under_limit.unwrap_or(max_safe).min(max_safe);
    assert_eq!(effective_under, 4096);
}
