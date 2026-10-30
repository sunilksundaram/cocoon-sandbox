# Cocoon 🛡️

[![CI](https://github.com/sunilksundaram/cocoon/actions/workflows/ci.yml/badge.svg)](https://github.com/sunilksundaram/cocoon/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/cocoon-sandbox.svg?color=orange&logo=rust)](https://crates.io/crates/cocoon-sandbox)
[![PyPI](https://img.shields.io/pypi/v/cocoon-sandbox.svg?color=blue&logo=pypi&logoColor=white)](https://pypi.org/project/cocoon-sandbox/)
[![Python Versions](https://img.shields.io/pypi/pyversions/cocoon-sandbox.svg?logo=python&logoColor=white)](https://pypi.org/project/cocoon-sandbox/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-green.svg)](#-license)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20Linux%20%7C%20macOS-blueviolet.svg)](#%EF%B8%8F-architecture--isolation-model)
[![Security Audit](https://img.shields.io/badge/security%20audit-RustSec%20passed-brightgreen.svg)](https://github.com/sunilksundaram/cocoon/actions/workflows/ci.yml)

> A featherweight, cross-platform sandbox with virtual and read-only filesystems designed for AI agents and secure code execution.

Cocoon allows AI agents, code evaluation suites, and developers to safely run untrusted scripts in **Python, Node.js, Bun, Deno, Go, Java, Rust, PowerShell, Bash, Ruby, PHP, Lua, and any binary compiler** without the latency or complexity of heavyweight containers or virtual machines.

👉 **[Installation Guide & Prerequisites](docs/INSTALLATION.md)** | **[User & Agent Integration Guide](docs/USER_GUIDE.md)**

---

## ⚡ Features

* **Sub-Millisecond Startup:** Runs directly on native operating system primitives (< 5ms) instead of spinning up virtual machines.
* **Ironclad Read-Only Protection:** Mount host directories with `--ro` where writes, overwrites, appends, and deletes are physically blocked by kernel ACLs / namespaces.
* **Ephemeral Virtual Scratchpad:** Automatically redirects temporary files, bytecode caches (`__pycache__`), npm caches, and cargo targets to an isolated in-memory/temp scratchpad.
* **Rich Built-in Runtime Presets:** Instant support for `python`, `node`, `bun`, `deno`, `go`, `java`, `rust` / `cargo`, `pwsh` / `powershell`, `bash` / `sh`, `ruby`, `php`, and `lua`.
* **Universal Command Execution:** Run any installed compiler or binary toolchain (`gcc`, `clang`, `dotnet`, `zig`, `julia`, etc.) by passing trailing arguments: `cocoon run -- <cmd> [args...]`.
* **Process & Resource Caps:** Hard limits on memory (RAM) and execution wall-clock timeouts.
* **Agent-First JSON Protocol:** Emits structured output (`--json`) with exit codes, execution duration, and captured standard streams.

---

## 🏗️ Architecture & Isolation Model

```
                    ┌────────────────────────────┐
                    │    AI Agent / Host App     │
                    └─────────────┬──────────────┘
                                  │ (CLI / Rust / JSON)
                    ┌─────────────▼──────────────┐
                    │       Cocoon Engine        │
                    └─────────────┬──────────────┘
                                  │
         ┌────────────────────────┼────────────────────────┐
         │                        │                        │
┌────────▼─────────┐    ┌─────────▼────────┐    ┌──────────▼─────────┐
│   Linux Driver   │    │   macOS Driver   │    │   Windows Driver   │
├──────────────────┤    ├──────────────────┤    ├────────────────────┤
│ • User Namespaces│    │ • Apple Seatbelt │    │ • Job Objects      │
│ • ro-bind mounts │    │ • SBPL Profile   │    │ • Inherited Deny   │
│ • Landlock LSM   │    │ • Ephemeral /tmp │    │   NTFS ACEs        │
│ • tmpfs scratch  │    │                  │    │ • Scratchpad VFS   │
└──────────────────┘    └──────────────────┘    └────────────────────┘
```

---

## 📦 Installation

Install Cocoon using your preferred package manager:

```bash
# Via Python / PyPI (Pre-compiled native binary, zero Rust toolchain needed)
pip install cocoon-sandbox

# Via Rust / Cargo (From Crates.io)
cargo install cocoon-sandbox
```

---

## 🚀 Quick Start

### 1. Inspect System Capabilities
```bash
cocoon info
```

### 2. Run Python with Read-Only Mount
Mount your project directory as read-only and execute an inspection script:
```bash
cocoon run --ro ./my_project --runtime python -c "import os; print(os.listdir('.'))"
```

### 3. Run Node.js with Memory & Timeout Limits
```bash
cocoon run --ro ./data --runtime node -c "console.log('Processed:', 42 * 2)" --timeout 5 --mem 256
```

### 4. Run PowerShell in Cocoon
```bash
cocoon run --ro ./scripts --runtime pwsh -c "Get-ChildItem"
```

### 5. Dedicated Work Directory (Create & Manipulate Files)
Specify an explicit workspace folder where scripts have full read-write permissions to create, manipulate, and delete files:
```bash
cocoon run --work ./agent_workspace --runtime python -c "open('report.txt', 'w').write('Analyzed')"
```

### 6. Safe Copy & Staging (Manipulate Copies While Leaving Originals Untouched)
Mount sensitive files/folders as read-only (`--ro`), and use `--copy` to stage copies into the writable work folder. The sandboxed process can edit and transform the files without altering the originals:
```bash
cocoon run --ro ./repo --copy ./repo/config.json --work ./agent_workspace \
  --runtime python -c "import json; d = json.load(open('config.json')); d['active'] = True; json.dump(d, open('config.json', 'w'))"
```

### 7. Custom Temp Directory
Redirect `TMP`, `TEMP`, and runtime bytecode caches to a specific directory:
```bash
cocoon run --temp ./my_scratch --runtime python -c "import os; print(os.environ['TMP'])"
```

### 8. Structured JSON Output for AI Agents
Add `--json` to receive structured execution telemetry:
```bash
cocoon run --ro . --runtime python -c "print('Computed:', 2**8)" --json
```

**Output:**
```json
{
  "exit_code": 0,
  "stdout": "Computed: 256\n",
  "stderr": "",
  "duration_ms": 184.2,
  "timed_out": false,
  "oom_killed": false
}
```

### 9. Multi-Language Polyglot Pipelines (`cocoon pipeline`)
Run sequential steps across **different programming languages in the exact same virtual filesystem**. Steps share the workspace and pass files between each other:
```bash
cocoon pipeline \
  -s "python:import json; json.dump({'agent':'Cocoon','score':99}, open('data.json','w'))" \
  -s "node:const fs=require('fs'); const d=JSON.parse(fs.readFileSync('data.json')); d.score+=1; fs.writeFileSync('data.json', JSON.stringify(d));" \
  -s "bun:const d=await Bun.file('data.json').json(); console.log('Bun verified score:', d.score);" \
  -s "pwsh:Get-Content data.json"
```

You can also pass `--json` to `cocoon pipeline` to get a structured execution breakdown per step:
```bash
cocoon pipeline \
  -s "python:open('val.txt','w').write('42')" \
  -s "bun:console.log('Value read by Bun:', (await Bun.file('val.txt').text()).trim())" \
  --json
```

---

## 🔒 Security Guarantees & Built-in Invariants

1. **Read-Only by Default (`--ro .`):**
   * Even if you omit `--ro`, the current working directory is automatically protected as **strictly read-only**.
   * Opening files in write mode (`'w'`, `'a'`) or attempting deletions triggers immediate `PermissionError` / `EPERM` enforced at the OS kernel level.
2. **Zero Host Writes (Workdir Isolation):**
   * Sandboxed processes can **never** write to anything other than the designated `--work` folder (or the auto-cleaned ephemeral scratchpad).
   * Use `--copy <path>` to stage copies of host files into the work folder and manipulate them freely without touching originals.
3. **30-Second Execution Timeout by Default:**
   * Every execution is automatically capped at **30 seconds** by default (override with `--timeout <secs>`). Runaway scripts or infinite loops auto-terminate cleanly without hanging.
4. **No Bytecode or Cache Pollution:**
   * Python bytecode caching (`PYTHONPYCACHEPREFIX`), pip caches (`PIP_CACHE_DIR`), npm caches (`npm_config_cache`), and cargo build artifacts are redirected to an ephemeral scratchpad that is destroyed upon exit.

---

## 📦 Project Workspace

* [`crates/cocoon-core`](crates/cocoon-core): Core interfaces, configuration schema, and traits.
* [`crates/cocoon-fs`](crates/cocoon-fs): Virtual filesystem views, ephemeral scratchpads, and read-only guards.
* [`crates/cocoon-platform`](crates/cocoon-platform): Native sandbox drivers (Windows Job Objects & ACLs, Linux Namespaces & Bubblewrap, macOS Seatbelt).
* [`crates/cocoon-runtimes`](crates/cocoon-runtimes): Auto-resolvers and isolation presets for Python, Node, PowerShell, Bash, and Rust.
* [`crates/cocoon-cli`](crates/cocoon-cli): The `cocoon` command-line executable.

---

## 📄 License

Dual-licensed under either:
* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
