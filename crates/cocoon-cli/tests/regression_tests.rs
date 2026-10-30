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
    // Invariant: Python preset must always include bytecode cache suppression flag (-B)
    let (_, args) =
        RuntimePreset::configure(RuntimeKind::Python, Some("print('regression test')"), None)
            .expect("Python configure should succeed");

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
