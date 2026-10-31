# Cocoon Integration & User Guide 📖

> A complete guide for using Cocoon as a standalone sandbox, with major agent frameworks (Microsoft Agent Framework, LangChain, LangGraph, LlamaIndex), and agent harnesses (Claude Desktop, GitHub Copilot, Hermes, Cursor, Antigravity, and custom agent loops).

👉 *Need setup or prerequisite instructions? See the [Installation Guide](INSTALLATION.md) and the [Security & Vulnerability Remediation Guide](SECURITY_AND_VULNERABILITIES.md).*

---

## ⚡ Why Cocoon for AI Agents?

Cocoon is purpose-built for AI agents and code execution:
* **Zero Host Risk:** Current directory is read-only (`--ro .`) by default; writes are strictly jailed to `--work`.
* **Sub-Millisecond Startup:** No container or VM initialization latency (< 5ms) — runs directly on native OS isolation primitives (Windows Job Objects/ACLs, Linux Namespaces/cgroups, macOS Seatbelt).
* **Automated 30-Second Timeout:** Runaway scripts or infinite loops are terminated automatically.
* **Agent-First JSON Telemetry:** Emits structured output (`--json`) with exit codes, execution duration, and standard streams.
* **Polyglot:** Built-in presets for Python, Node.js, Bun, Deno, Go, Java, Rust/Cargo, PowerShell, Bash, Ruby, PHP, and Lua.

---

## 1. Standalone CLI Usage

### Quick Reference
```bash
# 1. Safe execution (current directory is read-only, default-deny network, 30s timeout)
cocoon run -r python -c "print(2 + 2)"

# 2. Run inside an explicit writable workspace with disk quota
cocoon run --work ./my_workspace --disk 256 -r node -c "require('fs').writeFileSync('out.txt', 'done')"

# 3. Copy-on-Write: Stage repository files into workspace and manipulate them safely
cocoon run --copy ./config.json --work ./workspace -r python -c "
import json
d = json.load(open('config.json'))
d['active'] = True
json.dump(d, open('config.json', 'w'))
"

# 4. Opt-in network access (when task explicitly requires outbound HTTP requests)
cocoon run --allow-network -r node -c "console.log('Fetching external API...')"

# 5. Polyglot Multi-Language Pipeline (Python -> Node -> Bun in the SAME virtual workspace)
cocoon pipeline \
  -s "python:open('data.txt', 'w').write('hello from python')" \
  -s "node:console.log('Read by Node:', require('fs').readFileSync('data.txt', 'utf8'))" \
  -s "bun:console.log('Read by Bun:', await Bun.file('data.txt').text())" \
  --json

# 6. Structured SIEM JSON Logging
cocoon --log-json run -r python -c "print('Audit log event')"

# 7. Inspect available runtimes on host
cocoon info
```

---

## 2. Universal Tool Definition (OpenAI / JSON-RPC Schema)

Any agent harness using standard function calling can use this tool schema:

```json
{
  "name": "cocoon_run",
  "description": "Safely execute code in an isolated sandbox with read-only host protection and a 30s timeout. Writes are only permitted in the designated work folder.",
  "parameters": {
    "type": "object",
    "properties": {
      "runtime": {
        "type": "string",
        "enum": ["python", "python-skill", "node", "bun", "deno", "go", "java", "rust", "pwsh", "bash", "ruby", "php", "lua"],
        "description": "The programming language runtime to execute. Use 'python' for untrusted code (stdlib only) or 'python-skill' for format packages (openpyxl, docx)."
      },
      "code": {
        "type": "string",
        "description": "Inline code snippet to execute."
      },
      "workdir": {
        "type": "string",
        "description": "Optional directory for writing outputs. If omitted, uses an ephemeral scratchpad."
      },
      "copy_paths": {
        "type": "array",
        "items": { "type": "string" },
        "description": "Paths to copy into the work folder before execution for safe modification."
      },
      "timeout_seconds": {
        "type": "integer",
        "default": 30,
        "description": "Maximum allowed execution time in seconds."
      },
      "allow_network": {
        "type": "boolean",
        "default": false,
        "description": "Whether to permit outbound HTTP/network access. Defaults to false (blocked) to prevent credential exfiltration."
      },
      "max_processes": {
        "type": "integer",
        "default": 32,
        "description": "Maximum active process cap to prevent fork bombs."
      },
      "disk_mb": {
        "type": "integer",
        "default": 512,
        "description": "Disk write quota in Megabytes for the workspace."
      }
    },
    "required": ["runtime", "code"]
  }
}
```

---

## 3. Microsoft Agent Framework (AutoGen & Semantic Kernel)

### A. Microsoft AutoGen
In AutoGen, register Cocoon as a custom tool:

```python
import json
import subprocess
from autogen_agentchat.agents import AssistantAgent
from autogen_core.tools import FunctionTool

def execute_sandboxed_code(
    runtime: str,
    code: str,
    workdir: str = None,
    timeout: int = 30
) -> str:
    """Executes code inside the secure Cocoon sandbox."""
    cmd = ["cocoon", "run", "-r", runtime, "-c", code, "--json"]
    if workdir:
        cmd.extend(["--work", workdir])
    if timeout:
        cmd.extend(["--timeout", str(timeout)])
    
    proc = subprocess.run(cmd, capture_output=True, text=True)
    try:
        data = json.loads(proc.stdout)
        if data["exit_code"] != 0:
            return f"Execution Failed (exit {data['exit_code']}):\n{data['stderr']}"
        return data["stdout"]
    except Exception:
        return proc.stdout or proc.stderr

cocoon_tool = FunctionTool(execute_sandboxed_code, description="Run sandboxed code with Cocoon")
agent = AssistantAgent("coder", model_client=model_client, tools=[cocoon_tool])
```

### B. Microsoft Semantic Kernel
```python
import json
import subprocess
from semantic_kernel.functions import kernel_function

class CocoonPlugin:
    @kernel_function(
        description="Executes code safely in a sandbox with read-only host protection."
    )
    def run_code(self, runtime: str, code: str) -> str:
        res = subprocess.run(
            ["cocoon", "run", "-r", runtime, "-c", code, "--json"],
            capture_output=True,
            text=True
        )
        data = json.loads(res.stdout)
        return data["stdout"] if data["exit_code"] == 0 else data["stderr"]

kernel.add_plugin(CocoonPlugin(), plugin_name="Cocoon")
```

---

## 4. LangChain & LangGraph

### A. LangChain Tool
```python
import json
import subprocess
from langchain_core.tools import tool

@tool
def run_sandboxed(runtime: str, code: str, work: str = None, timeout: int = 30) -> str:
    """Execute code safely in the Cocoon sandbox (Python, Node, Bun, Go, PowerShell, Bash, etc.).
    Host files are strictly read-only by default.
    """
    cmd = ["cocoon", "run", "-r", runtime, "-c", code, "--json"]
    if work:
        cmd.extend(["--work", work])
    if timeout:
        cmd.extend(["--timeout", str(timeout)])
        
    res = subprocess.run(cmd, capture_output=True, text=True)
    output = json.loads(res.stdout)
    if output["exit_code"] != 0:
        return f"Error (code {output['exit_code']}): {output['stderr']}"
    return output["stdout"]
```

### B. LangGraph Code-Interpreter Agent Node
```python
import json
from typing import TypedDict, Annotated
import operator
from langgraph.graph import StateGraph, END

class AgentState(TypedDict):
    messages: Annotated[list, operator.add]
    code: str
    result: str

def execute_node(state: AgentState):
    # Runs the LLM-generated code in Cocoon
    result_str = run_sandboxed.invoke({"runtime": "python", "code": state["code"]})
    return {"result": result_str}

workflow = StateGraph(AgentState)
workflow.add_node("executor", execute_node)
```

---

## 5. LlamaIndex

```python
import json
import subprocess
from llama_index.core.tools import FunctionTool
from llama_index.core.agent import ReActAgent

def run_code_in_cocoon(runtime: str, code: str) -> str:
    """Safely runs code in a sandboxed Cocoon environment."""
    proc = subprocess.run(
        ["cocoon", "run", "-r", runtime, "-c", code, "--json"],
        capture_output=True,
        text=True
    )
    data = json.loads(proc.stdout)
    return data["stdout"] if data["exit_code"] == 0 else data["stderr"]

cocoon_tool = FunctionTool.from_defaults(fn=run_code_in_cocoon)
agent = ReActAgent.from_tools([cocoon_tool], llm=llm, verbose=True)
```

---

## 6. Model Context Protocol (MCP) for Claude Desktop, Cursor & Antigravity

Expose Cocoon as a local **MCP Server** so desktop environments and assistants can invoke it natively:

### Configuration in `claude_desktop_config.json` / `mcp_config.json`:
```json
{
  "mcpServers": {
    "cocoon": {
      "command": "python",
      "args": ["-m", "cocoon_mcp"]
    }
  }
}
```

### Minimal Python FastMCP Server (`cocoon_mcp.py`):
```python
import json
import subprocess
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("Cocoon Sandbox")

@mcp.tool()
def execute(
    runtime: str, 
    code: str, 
    timeout: int = 30, 
    allow_network: bool = False,
    max_procs: int = 32,
    disk_mb: int = 512
) -> str:
    """Run sandboxed code in Python, Node, Bun, Go, PowerShell, etc. Host files are read-only."""
    cmd = [
        "cocoon", "run", "-r", runtime, "-c", code,
        "--timeout", str(timeout),
        "--max-procs", str(max_procs),
        "--disk", str(disk_mb),
        "--json"
    ]
    if allow_network:
        cmd.append("--allow-network")
    p = subprocess.run(cmd, capture_output=True, text=True)
    return p.stdout

if __name__ == "__main__":
    mcp.run()
```

---

## 7. Hermes & Custom OpenAI-Compatible Agent Loops

For harnesses like **Nous Hermes**, **OpenAI Assistant API**, or lightweight custom agent loops:

```python
import json
import subprocess
from openai import OpenAI

client = OpenAI()

tools = [{
    "type": "function",
    "function": {
        "name": "cocoon_run",
        "description": "Execute code in a secure sandbox. Host files are strictly read-only.",
        "parameters": {
            "type": "object",
            "properties": {
                "runtime": {
                    "type": "string",
                    "enum": ["python", "node", "bun", "pwsh", "bash", "go"]
                },
                "code": {"type": "string"}
            },
            "required": ["runtime", "code"]
        }
    }
}]

def handle_tool_call(call):
    args = json.loads(call.function.arguments)
    proc = subprocess.run(
        ["cocoon", "run", "-r", args["runtime"], "-c", args["code"], "--json"],
        capture_output=True,
        text=True
    )
    return proc.stdout

# In your agent reasoning loop:
response = client.chat.completions.create(
    model="hermes-3-llama-3.1-8b",
    messages=messages,
    tools=tools
)

if response.choices[0].message.tool_calls:
    for tool_call in response.choices[0].message.tool_calls:
        if tool_call.function.name == "cocoon_run":
            result_json = handle_tool_call(tool_call)
            # Feed result_json back into agent message history
```

---

## 8. Summary of Key Invariants

1. **Host Immutability:** Never worry about `rm -rf` or file corruption — the agent only has write rights in `--work`.
2. **Infinite Loop Protection:** Default 30s timeout terminates the process tree at the OS level.
3. **Multi-Turn State:** Pass the same `--work ./session_dir` across conversation turns to maintain state and files between reasoning steps.

---

## 9. Sandbox Defaults & Security Controls: Turning Settings On/Off 🛡️

Cocoon ships with strict **production-grade security defaults** designed to isolate untrusted, model-generated code without container or VM startup latency. Every default can be tuned via CLI flags or programmatic SDK configuration.

### Summary of System Defaults

| Security Setting | Default Value | CLI Flag to Override | Primary Protection |
| :--- | :--- | :--- | :--- |
| **Network Access** | `false` (Blocked) | `--allow-network` | Prevents outbound data/credential exfiltration, SSRF to localhost chassis APIs, and Cloud Metadata theft (`169.254.169.254`). |
| **Execution Timeout** | `30 seconds` | `--timeout <SECS>` | Prevents runaway infinite loops, hanging background tasks, and CPU starvation. |
| **Active Process Cap** | `32 processes` | `--max-procs <COUNT>` | Prevents fork bombs and OS process table/handle exhaustion via Job Objects / cgroups. |
| **Disk Write Quota** | `512 MB` | `--disk <MB>` | Prevents disk exhaustion denial-of-service (DoS) from loop write attacks. |
| **Memory (RAM) Cap** | `Unlimited` (Host RAM) | `--mem <MB>` | Optional hardware RAM limit; terminates process if exceeded. |
| **Output Buffer Limit** | `10 MB` per stream | *Hard limit* | Prevents host OOM crashes from infinite `stdout`/`stderr` print loops (`stdout_truncated`). |
| **Filesystem Access** | Current dir is `ReadOnly` | `--ro <PATH>`, `--rw <PATH>` | Host files cannot be modified or deleted. Writes restricted to ephemeral `--work`. |
| **Environment Profile** | Isolated `dummy_user` | `--inherit-env` | Host credentials in `~/.aws`, `~/.ssh`, `~/.azure`, and Copilot are completely hidden. |
| **Python Isolation** | `-I -S -B` (stdlib only) | `-r python-skill` | Untrusted code cannot access host site-packages, user site, or install pip packages. |
| **Compiled Toolchains** | Offline mode enforced | `--allow-network` | Cargo enforces `--offline`; Go enforces `-mod=readonly` and `GOPROXY=off`. |
| **Breakout Detection** | Active post-run check | *Automatic* | Traverses workspace to detect and block symlinks or NTFS directory junctions escaping to host files. |

---

### Detailed Breakdown: Effects of Toggling Settings

#### 1. Network Access (`--allow-network`)
* **Default:** `false` (Offline / Blocked)
* **How it works:**
  - **Windows:** Proxies are nullified (`127.0.0.1:0`), environment variables for networking are blocked, and AppContainer mode strips all network capabilities at the kernel TCP/IP stack (`WSAEACCES`).
  - **Linux:** Bubblewrap executes `--unshare-all` without `--share-net`. Network interfaces (including loopback) are completely disconnected.
* **Turning it ON (`--allow-network`):**
  - *Functionality gained:* The sandboxed process can make outbound HTTP/HTTPS requests, download dependencies, scrape web pages, and connect to remote APIs.
  - *Security risks:* Untrusted code can exfiltrate sensitive files, call localhost dev servers/chassis APIs, or reach Cloud Metadata (`http://169.254.169.254/`).
* **Turning it OFF (Default):**
  - Zero data exfiltration risk. Sockets fail immediately with permission denied.

```bash
# Default: Sockets fail immediately
cocoon run -r python -c "import urllib.request; urllib.request.urlopen('https://example.com')"
# Error: Connection blocked / refused

# Opt-in network access:
cocoon run --allow-network -r python -c "import urllib.request; print(urllib.request.urlopen('https://httpbin.org/ip').read().decode())"
```

---

#### 2. Execution Timeout (`--timeout <SECS>`)
* **Default:** `30 seconds`
* **How it works:** Enforced via high-precision monotonic timers. On timeout:
  - **Windows:** Calls `TerminateJobObject`, atomically killing the entire process tree (all children and grandchildren).
  - **Linux:** Kills the process and Bubblewrap namespace container.
  - Telemetry records `timed_out: true` and `termination_reason: "timeout"`.
* **Increasing / Disabling Timeout:**
  - *When to increase:* Long-running batch tasks, extensive data transformations, or compilations (e.g. `--timeout 300` for 5 minutes).
  - *Security risks:* A model hallucinating `while True: pass` will monopolize a CPU core for the entire duration.

```bash
# Allow a heavy computation up to 2 minutes
cocoon run --timeout 120 -r node -f heavy_analytics.js
```

---

#### 3. Active Process Limit (`--max-procs <COUNT>`)
* **Default:** `32 processes`
* **How it works:**
  - **Windows:** Enforced via `JOB_OBJECT_LIMIT_ACTIVE_PROCESS` on the Windows Job Object.
  - **Linux:** Enforced via `RLIMIT_NPROC` (`setrlimit`).
* **Turning it Up / Off:**
  - *When to increase:* Multi-threaded build tools or complex test runners that spawn dozens of worker subprocesses (e.g., `--max-procs 128`).
  - *Security risks:* Allows fork bombs (e.g., `while True: os.fork()`) to exhaust system PIDs and hang the host OS.
* **Default behavior:** Any fork or subprocess call beyond 32 immediately fails with `Resource temporarily unavailable`.

```bash
# Constrain untrusted code strictly to 4 processes
cocoon run --max-procs 4 -r python -c "import multiprocessing; ..."
```

---

#### 4. Disk Write Quota (`--disk <MB>`)
* **Default:** `512 MB`
* **How it works:**
  - **Linux:** Enforces `RLIMIT_FSIZE` at the kernel layer. Any file write that exceeds the quota throws `EFBIG` (`File too large`).
  - **Cross-Platform:** Traverses the ephemeral scratchpad upon execution teardown; fails with `CocoonError::SecurityViolation` if total written size exceeds quota.
* **Turning it Up:**
  - *When to increase:* Image/video generation, processing large Parquet/CSV files, or compiling Rust binaries (e.g., `--disk 2048` for 2 GB).
  - *Security risks:* Scripts writing garbage data in a loop can fill the host hard drive.

```bash
# Allow up to 2GB of disk writes for artifact generation
cocoon run --disk 2048 -r python -f generate_video_frames.py
```

---

#### 5. Memory Allocation Limit (`--mem <MB>`)
* **Default:** `None` (unlimited, capped by host RAM)
* **How it works:**
  - **Windows:** Enforced via `JOB_OBJECT_LIMIT_JOB_MEMORY`. If committed memory crosses the boundary, the Windows kernel aborts the process and flags `oom_killed: true`.
  - **Linux:** Enforced via `RLIMIT_AS` (`libc::setrlimit`).
* **Turning it ON (Setting `--mem <MB>`):**
  - *Functionality:* Guarantees that an agent script with a memory leak or memory bomb (`"A" * 10**10`) cannot crash other host services or starve the agent chassis.

```bash
# Cap memory strictly at 256 MB
cocoon run --mem 256 -r node -e "let a = []; while(true) a.push(new Array(1e6))"
# Automatically terminated when 256MB is reached
```

---

#### 6. Hermetic Language Runtime Hardening
To prevent untrusted scripts from tapping into pre-installed tools, global plugins, or host startup scripts, Cocoon applies hermetic flags and environment sanitization across every supported language preset:

* **Python (`python` vs. `python-skill`):**
  - **`python` (Default):** Isolated Mode (`python -I -S -B`). Disables host `PYTHONPATH`/`PYTHONHOME` (`-I`), denies third-party `site-packages` (`-S`), and disables bytecode writes (`-B`).
  - **`python-skill`:** Invokes `python -B` without `-S`, allowing access to trusted system format packages (e.g. `openpyxl`, `python-docx`, `pandas`).
* **Node.js (`node`):**
  - Invokes `node --no-addons` to prevent loading arbitrary compiled C++ native binary add-ons (`.node`).
  - Automatically clears `NODE_OPTIONS` from the environment to prevent runtime flag injection attacks.
* **Ruby (`ruby`):**
  - Invokes `ruby --disable-gems` (Ruby's direct equivalent of Python's `-S`). Disables RubyGems initialization, restricting scripts strictly to the standard library.
  - Clears `RUBYOPT` from the environment.
* **PHP (`php`):**
  - Invokes `php -n` (No configuration file). Ignores any host `php.ini` files to block `auto_prepend_file` or dangerous extension preloads.
* **Deno (`deno`):**
  - Passes `--no-prompt` on both `deno eval` and `deno run` to prevent the child process from hanging indefinitely on interactive permission requests.
* **Bun (`bun`):**
  - Passes `--no-install` to ensure Bun never attempts to auto-download missing packages from npm during script execution.
* **PowerShell (`pwsh`):**
  - Invokes `-NoProfile -NonInteractive -ExecutionPolicy Bypass` to prevent executing host `profile.ps1` startup scripts.
* **Bash (`bash`):**
  - Invokes `--noprofile --norc` to prevent loading `.bashrc` or `.bash_profile`.
* **Rust & Go:**
  - Rust invokes `cargo script --offline` and injects `CARGO_NET_OFFLINE=true`.
  - Go invokes `go run -mod=readonly` and injects `GOPROXY=off`, `GONOSUMDB=*`.

| Runtime | Hermetic Hardening Flags | Environment Cleared / Injected |
| :--- | :--- | :--- |
| **Python** | `-I -S -B` | `PYTHONNOUSERSITE=1`, `PIP_NO_INDEX=1`, `PYTHONUSERBASE=empty` |
| **Python Skill** | `-B` | `PIP_NO_INDEX=1`, `PYTHONUSERBASE=empty` |
| **Node.js** | `--no-addons` | `NODE_OPTIONS=""`, `npm_config_offline=true` |
| **Ruby** | `--disable-gems` | `RUBYOPT=""` |
| **PHP** | `-n` | Host `php.ini` bypassed |
| **Deno** | `--no-prompt` | Terminal prompts disabled |
| **Bun** | `--no-install` | Auto-install disabled |
| **PowerShell** | `-NoProfile -NonInteractive` | Host profiles ignored |
| **Bash** | `--noprofile --norc` | Host rc files ignored |
| **Rust** | `cargo script --offline` | `CARGO_NET_OFFLINE=true` |
| **Go** | `go run -mod=readonly` | `GOPROXY=off`, `GONOSUMDB=*` |

```bash
# 1. Untrusted Python: Runs safely with stdlib only
cocoon run -r python -c "import math; print(math.sqrt(16))"

# 2. Document skill: Runs with openpyxl/docx support
cocoon run -r python-skill -c "import openpyxl; print('Excel engine ready')"

# 3. Hermetic Node.js: Addons disabled and env options neutralized
cocoon run -r node -c "console.log('Hermetic JS Execution')"
```

---

#### 7. Filesystem Boundaries: Read-Only vs. Read-Write Mounts
* **Default:** Current directory (`.`) is mounted as `ReadOnly` (`--ro`).
* **Jail Execution:** All file creations, edits, and temp caches take place inside an ephemeral workspace folder, destroyed on exit.
* **Mount Controls:**
  - `--ro <PATH>`: Mount a specific host directory as strictly Read-Only.
  - `--rw <PATH>`: Grant explicit Read-Write access to a designated folder.
  - `--copy <PATH>`: Copy-on-Write staging: copies files into the scratchpad so scripts can modify them without touching the originals.

```bash
# Safe testing: inspect repository code without risk of accidental file modification
cocoon run --ro ./src --work ./scratch -r python -c "
import os
for root, dirs, files in os.walk('.'):
    print(files)
"
```

---

#### 8. Structured SIEM Logging (`--log-json`)
* **Default:** Human-readable warnings and status output on `stderr`.
* **Turning it ON (`--log-json`):**
  - Formats all internal events (startup, security warnings, breakout alerts, teardown) as single-line JSON records formatted for Datadog, Splunk, CloudWatch, or Google Cloud Operations.

```bash
cocoon --log-json run -r node -c "console.log('App started')"
```
Sample log output:
```json
{"timestamp":"2026-10-30T16:22:15.102Z","level":"INFO","target":"cocoon::runtime","fields":{"message":"Spawning sandboxed process","executable":"node"}}
{"timestamp":"2026-10-30T16:22:15.115Z","level":"WARN","target":"cocoon::security","fields":{"event":"security_violation","violation_type":"junction_escape","message":"Detected directory junction breakout pointing outside workspace"}}
```

---

#### 9. Desktop & Clipboard Isolation (Win32 UI Restrictions)
* **Default:** Enforced automatically on Windows via Win32 Job Object UI Restrictions (`JobObjectBasicUIRestrictions`).
* **Protections:**
  - `JOB_OBJECT_UILIMIT_READCLIPBOARD`: Disallows child processes from calling `GetClipboardData` or PowerShell `Get-Clipboard`. Prevents stealing passwords or tokens from the developer's clipboard.
  - `JOB_OBJECT_UILIMIT_WRITECLIPBOARD`: Prevents untrusted scripts from poisoning or altering the clipboard.
  - `JOB_OBJECT_UILIMIT_HANDLES`: Disallows access to USER and GDI handles belonging to processes outside the job, preventing Shatter attacks and window hook injections.
  - `JOB_OBJECT_UILIMIT_DESKTOP`: Blocks creating or switching desktop viewports.

---

#### 10. Universal Breakout Verification & CWD DLL Defense
* **Default:** Active on every execution teardown.
* **Protections:**
  - **Universal Canonicalization:** Recursively verifies every entry in the workspace upon completion. Any path (symbolic link, Windows NTFS directory junction `0x400`, or hardlink) resolving outside the workspace root triggers an immediate `CocoonError::SecurityViolation`.
  - **CWD DLL Search Blocking:** Injects `CWDIllegalInDllSearch=0x2` to eliminate DLL preloading attacks from untrusted workspaces on Windows.

---

#### 11. Host IPC & Credential Variable Purging
* **Default:** Enforced on every execution.
* **Protections:**
  - Automatically neutralizes dangerous host IPC pointers: `DOCKER_HOST` (`\\.\pipe\docker_engine`), `SSH_AUTH_SOCK` (OpenSSH agent pipe), `WSL_INTEROP`, and `GPG_AGENT_INFO`.
  - Automatically purges cloud credentials: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `GITHUB_TOKEN`, `GH_TOKEN`, `AZURE_CONFIG_DIR`, and `KUBECONFIG`.
  - Even if `inherit_env: true` is configured, these sensitive endpoints are never passed to the sandboxed child.

---

### Common Recipe Profiles

#### Recipe A: Untrusted AI Agent Code (Maximum Lockdown)
*Use when executing arbitrary code generated by an LLM.*
```bash
# Maximum security: No network, 15s timeout, 128MB RAM, 100MB disk, stdlib only
cocoon run \
  --timeout 15 \
  --mem 128 \
  --disk 100 \
  --max-procs 8 \
  -r python \
  -c "print('Running untrusted code safely')" \
  --json
```

#### Recipe B: Data Analysis & Document Generation
*Use when an agent needs to process data or build Excel/Word reports.*
```bash
# Enabled libraries, isolated disk, 60s timeout, offline
cocoon run \
  --work ./output_reports \
  --timeout 60 \
  --disk 500 \
  -r python-skill \
  -f generate_monthly_report.py
```

#### Recipe C: Web Scraping & API Integration Agent
*Use when an agent is explicitly tasked with fetching external information.*
```bash
# Network enabled, 256MB RAM, current directory read-only
cocoon run \
  --allow-network \
  --timeout 45 \
  --mem 256 \
  -r node \
  -c "
    const https = require('https');
    https.get('https://api.github.com/zen', { headers: { 'User-Agent': 'Cocoon' } }, res => {
      res.pipe(process.stdout);
    });
  "
```
