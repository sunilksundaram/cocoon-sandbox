use anyhow::Result;
use cocoon_core::{MountRule, MountType, ResourceLimits, SandboxConfig, SandboxDriver};
use cocoon_runtimes::{RuntimeKind, RuntimePreset};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::doctor::generate_doctor_report;

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

pub fn run_mcp_server(driver: Arc<dyn SandboxDriver>) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = JsonRpcResponse {
                    jsonrpc: "2.0",
                    id: Value::Null,
                    result: None,
                    error: Some(json!({
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    })),
                };
                let _ = writeln!(stdout, "{}", serde_json::to_string(&err_resp)?);
                let _ = stdout.flush();
                continue;
            }
        };

        let resp = handle_request(&req, driver.clone());
        if let Some(res) = resp {
            let out_str = serde_json::to_string(&res)?;
            writeln!(stdout, "{}", out_str)?;
            let _ = stdout.flush();
        }
    }

    Ok(())
}

fn handle_request(req: &JsonRpcRequest, driver: Arc<dyn SandboxDriver>) -> Option<JsonRpcResponse> {
    let id = req.id.clone().unwrap_or(Value::Null);

    match req.method.as_str() {
        "initialize" => Some(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "cocoon",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            error: None,
        }),

        "notifications/initialized" | "notifications/cancelled" => None,

        "ping" => Some(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({})),
            error: None,
        }),

        "tools/list" => Some(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(json!({
                "tools": [
                    {
                        "name": "execute_code",
                        "description": "Executes untrusted code in an ultra-low-latency secure sandbox. Read-only workspace, default-deny network, hard memory/CPU caps.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "code": {
                                    "type": "string",
                                    "description": "The source code or script to execute"
                                },
                                "runtime": {
                                    "type": "string",
                                    "enum": ["python", "node", "bun", "deno", "go", "rust", "java", "pwsh", "bash", "ruby", "php", "lua"],
                                    "default": "python",
                                    "description": "The language runtime preset to use"
                                },
                                "timeout": {
                                    "type": "integer",
                                    "default": 30,
                                    "description": "Timeout in seconds (max 300)"
                                },
                                "work_dir": {
                                    "type": "string",
                                    "description": "Optional host directory path to use as the writable workspace"
                                }
                            },
                            "required": ["code"]
                        }
                    },
                    {
                        "name": "execute_skill",
                        "description": "Executes document processing tasks (Excel openpyxl/pandas, Word python-docx, PDF) with read-only file mounts and zero host write risk.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "code": {
                                    "type": "string",
                                    "description": "Python script using pandas, openpyxl, python-docx, or pypdf"
                                },
                                "input_files": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Host file paths to mount as strictly read-only"
                                },
                                "work_dir": {
                                    "type": "string",
                                    "description": "Optional host directory path to store generated documents and artifacts"
                                },
                                "timeout": {
                                    "type": "integer",
                                    "default": 60,
                                    "description": "Timeout in seconds"
                                }
                            },
                            "required": ["code"]
                        }
                    },
                    {
                        "name": "inspect_runtimes",
                        "description": "Inspect detected programming language compilers and interpreters on the host system.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {}
                        }
                    },
                    {
                        "name": "doctor",
                        "description": "Runs environment diagnostics and verification checks on host sandbox drivers, filesystem isolation, and installed language toolchains.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {}
                        }
                    }
                ]
            })),
            error: None,
        }),

        "tools/call" => {
            let tool_name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let arguments = req.params.get("arguments").cloned().unwrap_or(json!({}));

            let result = match tool_name {
                "execute_code" => handle_execute_code(&arguments, driver),
                "execute_skill" => handle_execute_skill(&arguments, driver),
                "inspect_runtimes" => handle_inspect_runtimes(),
                "doctor" => handle_doctor(driver),
                _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
            };

            match result {
                Ok(content) => Some(JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(json!({
                        "content": [
                            {
                                "type": "text",
                                "text": content
                            }
                        ],
                        "isError": false
                    })),
                    error: None,
                }),
                Err(err) => Some(JsonRpcResponse {
                    jsonrpc: "2.0",
                    id,
                    result: Some(json!({
                        "content": [
                            {
                                "type": "text",
                                "text": format!("Error: {}", err)
                            }
                        ],
                        "isError": true
                    })),
                    error: None,
                }),
            }
        }

        _ => Some(JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(json!({
                "code": -32601,
                "message": format!("Method not found: {}", req.method)
            })),
        }),
    }
}

fn handle_execute_code(args: &Value, driver: Arc<dyn SandboxDriver>) -> Result<String> {
    let code = args
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'code' parameter"))?;

    let rt_str = args
        .get("runtime")
        .and_then(|v| v.as_str())
        .unwrap_or("python");

    let timeout_secs = args
        .get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(30)
        .min(300);

    let kind: RuntimeKind = rt_str.parse().map_err(|e| anyhow::anyhow!("{}", e))?;
    let (executable, cmd_args) = RuntimePreset::configure(kind, Some(code), None)?;

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mounts = vec![MountRule {
        host_path: cwd,
        guest_path: None,
        mount_type: MountType::ReadOnly,
    }];

    let working_dir = args
        .get("work_dir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);

    let limits = ResourceLimits {
        timeout: Some(Duration::from_secs(timeout_secs)),
        memory_limit_mb: Some(512),
        cpu_limit_pct: Some(95),
        allow_network: false,
        max_processes: Some(32),
        max_output_bytes: Some(10 * 1024 * 1024),
        disk_limit_mb: Some(256),
        allow_gpu: false,
    };

    let config = SandboxConfig {
        executable,
        args: cmd_args,
        working_dir,
        temp_dir: None,
        stage_copies: vec![],
        env: Default::default(),
        inherit_env: false,
        mounts,
        limits,
        stdin_data: None,
        stream_sender: None,
    };

    let result = driver
        .execute(&config)
        .map_err(|e| anyhow::anyhow!("Sandbox execution error: {}", e))?;

    let response = json!({
        "exit_code": result.exit_code,
        "stdout": result.stdout,
        "stderr": result.stderr,
        "duration_ms": result.duration_ms,
        "timed_out": result.timed_out,
        "oom_killed": result.oom_killed,
        "peak_memory_mb": result.peak_memory_mb,
        "termination_reason": result.termination_reason
    });

    Ok(serde_json::to_string_pretty(&response)?)
}

fn handle_execute_skill(args: &Value, driver: Arc<dyn SandboxDriver>) -> Result<String> {
    let code = args
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'code' parameter"))?;

    let timeout_secs = args
        .get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(60)
        .min(600);

    let (executable, cmd_args) =
        RuntimePreset::configure(RuntimeKind::PythonSkill, Some(code), None)?;

    let mut mounts = Vec::new();
    if let Some(files) = args.get("input_files").and_then(|v| v.as_array()) {
        for f in files {
            if let Some(p_str) = f.as_str() {
                mounts.push(MountRule {
                    host_path: PathBuf::from(p_str),
                    guest_path: None,
                    mount_type: MountType::ReadOnly,
                });
            }
        }
    }

    let working_dir = args
        .get("work_dir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);

    let limits = ResourceLimits {
        timeout: Some(Duration::from_secs(timeout_secs)),
        memory_limit_mb: Some(2048),
        cpu_limit_pct: Some(95),
        allow_network: false,
        max_processes: Some(32),
        max_output_bytes: Some(10 * 1024 * 1024),
        disk_limit_mb: Some(1024),
        allow_gpu: false,
    };

    let config = SandboxConfig {
        executable,
        args: cmd_args,
        working_dir,
        temp_dir: None,
        stage_copies: vec![],
        env: Default::default(),
        inherit_env: false,
        mounts,
        limits,
        stdin_data: None,
        stream_sender: None,
    };

    let result = driver
        .execute(&config)
        .map_err(|e| anyhow::anyhow!("Skill execution error: {}", e))?;

    let response = json!({
        "exit_code": result.exit_code,
        "stdout": result.stdout,
        "stderr": result.stderr,
        "duration_ms": result.duration_ms,
        "timed_out": result.timed_out,
        "oom_killed": result.oom_killed,
        "peak_memory_mb": result.peak_memory_mb,
    });

    Ok(serde_json::to_string_pretty(&response)?)
}

fn handle_doctor(driver: Arc<dyn SandboxDriver>) -> Result<String> {
    let report = generate_doctor_report(driver);
    Ok(serde_json::to_string_pretty(&report)?)
}

fn handle_inspect_runtimes() -> Result<String> {
    let mut detected = Vec::new();
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
        for &e in exes {
            if let Some(p) = cocoon_runtimes::which(e) {
                detected.push(json!({
                    "runtime": name,
                    "executable": p.to_string_lossy().to_string()
                }));
                break;
            }
        }
    }

    Ok(serde_json::to_string_pretty(&detected)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cocoon_platform::get_default_driver;

    #[test]
    fn test_mcp_initialize() {
        let driver = get_default_driver();
        let req = JsonRpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(1)),
            method: "initialize".into(),
            params: json!({}),
        };
        let resp = handle_request(&req, driver).expect("response");
        let result = resp.result.expect("result");
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "cocoon");
    }

    #[test]
    fn test_mcp_tools_list() {
        let driver = get_default_driver();
        let req = JsonRpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(2)),
            method: "tools/list".into(),
            params: json!({}),
        };
        let resp = handle_request(&req, driver).expect("response");
        let result = resp.result.expect("result");
        let tools = result["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 4);
        assert_eq!(tools[0]["name"], "execute_code");
        assert_eq!(tools[1]["name"], "execute_skill");
        assert_eq!(tools[2]["name"], "inspect_runtimes");
        assert_eq!(tools[3]["name"], "doctor");
    }

    #[test]
    fn test_mcp_doctor_call() {
        let driver = get_default_driver();
        let req = JsonRpcRequest {
            jsonrpc: Some("2.0".into()),
            id: Some(json!(3)),
            method: "tools/call".into(),
            params: json!({
                "name": "doctor",
                "arguments": {}
            }),
        };
        let resp = handle_request(&req, driver).expect("response");
        let result = resp.result.expect("result");
        assert_eq!(result["isError"], false);
        let content = result["content"].as_array().expect("content");
        assert!(!content.is_empty());
        let text = content[0]["text"].as_str().expect("text");
        assert!(text.contains("driver_name"));
    }
}
