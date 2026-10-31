# Cocoon-Sandbox 🛡️

[![CI](https://github.com/sunilksundaram/cocoon/actions/workflows/ci.yml/badge.svg)](https://github.com/sunilksundaram/cocoon/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/cocoon-sandbox.svg?color=orange&logo=rust)](https://crates.io/crates/cocoon-sandbox)
[![PyPI](https://img.shields.io/pypi/v/cocoon-sandbox.svg?color=blue&logo=pypi&logoColor=white)](https://pypi.org/project/cocoon-sandbox/)
[![Python Versions](https://img.shields.io/pypi/pyversions/cocoon-sandbox.svg?logo=python&logoColor=white)](https://pypi.org/project/cocoon-sandbox/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20Linux%20%7C%20macOS-blueviolet.svg)](#%EF%B8%8F-architecture--isolation-model)
[![Security Audit](https://img.shields.io/badge/security%20audit-RustSec%20passed-brightgreen.svg)](https://github.com/sunilksundaram/cocoon/actions/workflows/ci.yml)

> A featherweight, cross-platform sandbox with virtual and read-only filesystems designed for AI agents and secure code execution.

Cocoon allows AI agents, code evaluation suites, and developers to safely run untrusted scripts in **Python, Node.js, Bun, Deno, Go, Java, Rust, PowerShell, Bash, Ruby, PHP, Lua, and any binary compiler** without the latency or complexity of heavyweight containers or virtual machines.

👉 **[Installation Guide & Prerequisites](docs/INSTALLATION.md)** | **[User & Agent Integration Guide](docs/USER_GUIDE.md)** | **[Security & Vulnerability Remediation Guide](docs/SECURITY_AND_VULNERABILITIES.md)**

---

## ⚡ Features

* **Sub-Millisecond Startup:** Runs directly on native operating system primitives (< 5ms) instead of spinning up virtual machines.
* **Default-Deny Network:** Outbound network is disabled by default (`allow_network: false`). Linux unshares network namespaces (`CLONE_NEWNET`) for kernel-level socket severance; Windows sinks proxies (`127.0.0.1:0`) to block cooperative HTTP/HTTPS clients and package tools. Use `--allow-network` to opt in.
* **Read-Only Host Mounts:** Mount host directories with `--ro` where writes, overwrites, appends, and deletes are physically blocked by kernel mount namespaces on Linux and NTFS Deny ACEs on Windows.
* **Hermetic Runtimes & Dual-Mode Python:** Untrusted code runs in hermetic standard-library mode (`python -I -S -B`, `node --no-addons`, `ruby --disable-gems`), with a dedicated `python-skill` preset for document processing tools (`openpyxl`, `python-docx`, `pandas`).
* **Desktop & Clipboard Isolation:** Win32 Job Object UI restrictions prevent child processes from reading the host clipboard, injecting synthetic keystrokes, or intercepting window handles.
* **Resource & Process Caps:** Hard limits on active processes (`--max-procs 32`), disk write quotas (`--disk 512MB`), memory (`--mem`), and execution timeouts (`--timeout 30s`).
* **Ephemeral Virtual Scratchpad & User Profile:** Redirects caches (`__pycache__`, npm, cargo) and synthesizes a clean, isolated dummy user profile (`USERPROFILE`, `HOME`, `APPDATA`).
* **Workspace DACL Hardening:** Workspace permissions are strictly bounded to the executing user (`%USERNAME%`), never `Everyone` (`*S-1-1-0`).
* **Universal Breakout Verification:** Recursively scans workspace post-run to detect and reject symlinks, NTFS directory junctions (`0x400`), or hardlinks escaping the workspace root.
* **Universal Command Execution:** Run any installed compiler or binary toolchain (`gcc`, `clang`, `dotnet`, `zig`, `julia`, etc.) by passing trailing arguments: `cocoon run -- <cmd> [args...]`.
* **High-Throughput Heavy Data & Multimedia:** Native NVMe read speeds (3–7 GB/s) with zero virtualization overhead for multi-gigabyte Excel data (Pandas/Polars), multi-hundred page PDF conversions, and video/audio processing (`ffmpeg`).
* **Hardware-Aware 95% Ceiling & Host Protection:** Auto-detects host physical RAM and CPU cores across Windows, Linux, and macOS. Clamps resource usage to **95% MAX of physical hardware**, guaranteeing the host OS never freezes, thrashes, or blue-screens.
* **GPU VRAM & Driver Isolation (`--no-gpu`):** Multi-vendor driver detachment (NVIDIA CUDA, AMD ROCm/HIP, Intel oneAPI, OpenCL) via `--no-gpu`. Auto-injects PyTorch VRAM fragmentation guards (`max_split_size_mb:128`) when GPU is permitted for local SLMs and fine-tuning.
* **Process Priority & Core Dump Hardening:** Permanently locks process priority to `NORMAL_PRIORITY_CLASS` (Windows) and blocks real-time elevation (`RLIMIT_RTPRIO=0` on Linux) to prevent scheduler starvation. Suppresses crash core dumps (`RLIMIT_CORE=0`) to prevent disk flooding and prompt leaks.
* **Agent-First JSON Protocol & SIEM Logging:** Emits structured output (`--json`) and NDJSON event logs (`--log-json`) with exit codes, peak memory, output truncation, and termination reasons.

---

## 🏗️ Architecture & Isolation Model

```
                    ┌────────────────────────────┐
                    │    AI Agent / Host App     │
                    └─────────────┬──────────────┘
                                  │ (CLI / Rust / JSON / SIEM)
                    ┌─────────────▼──────────────┐
                    │       Cocoon Engine        │
                    └─────────────┬──────────────┘
                                  │
         ┌────────────────────────┼────────────────────────┐
         │                                                 │
┌────────▼─────────┐                             ┌─────────▼──────────┐
│   Linux Driver   │                             │   Windows Driver   │
├──────────────────┤                             ├────────────────────┤
│ • User Namespaces│                             │ • Job Objects      │
│ • ro-bind mounts │                             │ • UI Restrictions  │
│ • tmpfs /tmp     │                             │ • Inherited Deny   │
│ • tmpfs /dev/shm │                             │   NTFS ACEs        │
│ • rlimit caps    │                             │ • Scratchpad VFS   │
└──────────────────┘                             └────────────────────┘
             *(macOS Apple Seatbelt driver currently in development)*
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

### 2. Run Untrusted Python (Hermetic Standard Library)
Mount your project directory as read-only and execute an inspection script in isolated mode (`-I -S -B`):
```bash
cocoon run --ro ./my_project --runtime python -c "import os; print(os.listdir('.'))"
```

### 3. Run Document Format Skills (`python-skill`)
When running trusted format manipulation tools (e.g. `openpyxl`, `python-docx`, `pandas`), use the `python-skill` preset:
```bash
cocoon run --ro ./sheets --runtime python-skill -c "import openpyxl; print('Excel engine ready')"
```

### 4. Run Node.js with Memory & Timeout Limits
```bash
cocoon run --ro ./data --runtime node -c "console.log('Processed:', 42 * 2)" --timeout 5 --mem 256
```

### 5. Run PowerShell with Isolated Desktop
PowerShell automatically executes with `-NoProfile -NonInteractive -ExecutionPolicy Bypass` and clipboard isolation:
```bash
cocoon run --ro ./scripts --runtime pwsh -c "Get-ChildItem"
```

### 6. Dedicated Work Directory (Create & Manipulate Files)
Specify an explicit workspace folder where scripts have full read-write permissions to create, manipulate, and delete files:
```bash
cocoon run --work ./agent_workspace --runtime python -c "open('report.txt', 'w').write('Analyzed')"
```

### 7. Safe Copy & Staging (Manipulate Copies While Leaving Originals Untouched)
Mount sensitive files/folders as read-only (`--ro`), and use `--copy` to stage copies into the writable work folder. The sandboxed process can edit and transform the files without altering the originals:
```bash
cocoon run --ro ./repo --copy ./repo/config.json --work ./agent_workspace \
  --runtime python -c "import json; d = json.load(open('config.json')); d['active'] = True; json.dump(d, open('config.json', 'w'))"
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
  "oom_killed": false,
  "termination_reason": null,
  "stdout_truncated": false,
  "stderr_truncated": false,
  "peak_memory_mb": 14
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

### 10. Heavy Data & Document Processing (Pandas & Large Excel)
Mount large datasets as read-only (`--ro`) and process millions of rows at native NVMe bus speed without risk of file corruption:
```bash
cocoon run --ro ./data --work ./output --timeout 180 --disk 2048 -r python-skill -c "
import pandas as pd
df = pd.read_excel('data/massive_ledger.xlsx', engine='calamine')
df[df['amount'] > 50000].to_parquet('output/high_value.parquet')
print(f'Processed {len(df):,} rows successfully.')
" --json
```

### 11. Local SLM Inference & Fine-Tuning (GPU vs. `--no-gpu`)
Run local Small Language Models (SLMs) such as Phi-3, Llama-3-8B, or Mistral-7B with automatic PyTorch VRAM protection:
```bash
# GPU enabled with VRAM fragmentation protection:
cocoon run --ro ./models --work ./output --timeout 180 --disk 2048 -r python-skill -c "
from transformers import AutoModelForCausalLM, AutoTokenizer
model = AutoModelForCausalLM.from_pretrained('models/Phi-3-mini', device_map='auto')
print('Model ready for inference.')
" --json

# Or run strictly on CPU with complete GPU driver detachment:
cocoon run --no-gpu --ro ./models --work ./output --timeout 180 -r python-skill -c "print('CPU only')"
```

---

## 🔒 Security Guarantees & Built-in Invariants

1. **Default-Deny Network (`allow_network: false`):**
   * On Linux, network namespaces are unshared (`CLONE_NEWNET`), severing all sockets at the kernel level.
   * On Windows, proxies are sinkholed to `127.0.0.1:0`, blocking cooperative HTTP/HTTPS traffic. Opt-in with `--allow-network`.
2. **Read-Only by Default (`--ro .`):**
   * Even if you omit `--ro`, the current working directory is automatically protected as **strictly read-only**.
   * On Linux, mount namespaces enforce read-only status. On Windows, NTFS Deny ACEs block write/append/delete operations on designated folders (`PermissionError: Permission denied`). Ambient host paths outside `--ro` retain standard user read access.
3. **Workspace Access Control & Ephemeral Scratchpad:**
   * Writes are confined to the designated `--work` directory or auto-cleaned ephemeral scratchpad.
   * Workspace DACLs are strictly bounded to the executing user (`%USERNAME%`), eliminating legacy broad access (`Everyone`).
   * Use `--copy <path>` to stage copies of host files into the work folder and manipulate them freely without touching originals.
4. **Desktop & Clipboard Isolation:**
   * Win32 Job Object UI restrictions prevent child processes from reading the host clipboard (`JOB_OBJECT_UILIMIT_READCLIPBOARD`), modifying the clipboard, or sending synthetic window messages to other applications.
5. **30-Second Execution Timeout by Default:**
   * Every execution is automatically capped at **30 seconds** by default (override with `--timeout <secs>`). Runaway scripts or infinite loops auto-terminate cleanly via atomic process tree termination without hanging.
6. **Active Process Cap (`max_processes: 32`):**
   * Mitigates fork bombs and handle exhaustion by capping active child processes to 32 by default.
7. **Storage Quota Enforcement (`disk_limit_mb: 512MB`):**
   * Mitigates disk exhaustion attacks by capping workspace writes at 512 MB.
8. **No Bytecode or Cache Pollution:**
   * Python bytecode caching (`PYTHONPYCACHEPREFIX`), pip caches (`PIP_CACHE_DIR`), npm caches (`npm_config_cache`), and cargo build artifacts are redirected to an ephemeral scratchpad that is destroyed upon exit.
9. **Universal Breakout Verification:**
   * Post-run canonicalization scans detect and reject any symlink, NTFS junction (`0x400`), or hardlink attempting to point outside the workspace.
10. **Hardware-Aware 95% Ceiling:**
    * Auto-detects physical RAM and clamps memory allocations to `(Host Physical RAM * 0.95)`, mathematically preventing host paging thrashing or blue screens even if `--mem` is omitted.
11. **95% CPU Rate Cap & Scheduler Lockout:**
    * Enforces `NORMAL_PRIORITY_CLASS` (Windows) and `RLIMIT_RTPRIO=0` (Linux) to permanently block untrusted code from preempting host OS schedulers or system drivers. Clamps CPU rate to 95% MAX.
12. **GPU VRAM & Driver Isolation (`--no-gpu`):**
    * Strips and masks driver interfaces across NVIDIA, AMD ROCm/HIP, Intel, and OpenCL when `--no-gpu` is passed. Auto-injects `PYTORCH_CUDA_ALLOC_CONF="max_split_size_mb:128"` when GPU is enabled.
13. **Crash Core Dump Suppression:**
    * Injects `RLIMIT_CORE=0` on Linux and suppresses WER crash popups on Windows, eliminating multi-gigabyte core dumps and prompt memory leakage.

For a comprehensive breakdown of all threat models, CVE remediations, and OS defensive layers, see the **[Security & Vulnerability Remediation Guide](docs/SECURITY_AND_VULNERABILITIES.md)**.

---

## 📦 Project Workspace

* [`crates/cocoon-core`](crates/cocoon-core): Core interfaces, configuration schema, and traits.
* [`crates/cocoon-fs`](crates/cocoon-fs): Virtual filesystem views, ephemeral scratchpads, and read-only guards.
* [`crates/cocoon-platform`](crates/cocoon-platform): Native sandbox drivers (Windows Job Objects & ACLs, Linux Namespaces & Bubblewrap, macOS Seatbelt).
* [`crates/cocoon-runtimes`](crates/cocoon-runtimes): Auto-resolvers and isolation presets for Python, Node, Bun, Deno, Go, Rust, PowerShell, Bash, Ruby, and PHP.
* [`crates/cocoon-cli`](crates/cocoon-cli): The `cocoon` command-line executable.

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).
