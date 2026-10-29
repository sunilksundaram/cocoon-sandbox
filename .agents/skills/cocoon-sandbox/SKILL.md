---
name: cocoon-sandbox
description: >-
  Executes untrusted or dynamic code in an ultra-low-latency, sandboxed environment using Cocoon.
  Supports Python, Node.js, Bun, Deno, Go, Java, Rust/Cargo, PowerShell, Bash, Ruby, PHP, and Lua.
  Use when you need to safely run code, test scripts, parse data without mutating the host filesystem,
  run polyglot pipelines across multiple languages, or execute code with strict memory and timeout constraints.
---

# Cocoon Sandbox: Agent Execution Guide

`cocoon` is a native, featherweight cross-platform sandbox (< 5ms startup) that provides virtual and read-only filesystem views. It allows agents to safely execute code in **Python, Node.js, Bun, Deno, Go, Java, Rust, PowerShell, Bash, Ruby, PHP, Lua, or any installed compiler**.

*For framework integration patterns (AutoGen, LangChain, LangGraph, LlamaIndex, Claude/MCP, Hermes), see the [User & Integration Guide](../../../docs/USER_GUIDE.md).*

---

## 🎯 When to Use Cocoon

Use `cocoon` whenever you need to:
1. **Safely execute code:** Run generated code, user-provided scripts, or benchmark evaluations without risking the host system.
2. **Inspect data read-only:** Mount repositories or datasets with `--ro` to ensure files cannot be accidentally modified or deleted.
3. **Experiment with files safely (Copy-on-Write):** Use `--copy <path>` to stage a copy of host files into a writable workspace and manipulate them without altering the originals.
4. **Install local dependencies:** Install packages via `pip`, `npm`, `bun`, or PowerShell modules inside an isolated workspace without polluting the host or requiring admin.
5. **Run multi-language pipelines:** Chain together steps in different languages (e.g. Python $\to$ Node $\to$ Bun $\to$ PowerShell) sharing the exact same virtual workspace.
6. **Enforce hard limits:** Guard against infinite loops or runaway memory with `--timeout <secs>` and `--mem <mb>`.

---

## 🚀 Key Commands for Agents

### 1. Always Use `--json` for Machine Parsing
Agents should append `--json` to `cocoon run` or `cocoon pipeline` to receive structured execution telemetry:

```bash
cocoon run --ro . --runtime python -c "print(2 + 2)" --json
```

**JSON Response Schema:**
```json
{
  "exit_code": 0,
  "stdout": "4\n",
  "stderr": "",
  "duration_ms": 18.4,
  "timed_out": false,
  "oom_killed": false
}
```

---

### 2. Common Execution Patterns

#### Pattern A: Read-Only Repository Inspection
Mount the codebase as strictly read-only (`--ro`). Attempts to write or delete will fail immediately at the filesystem level:
```bash
cocoon run --ro ./my_project --runtime python -c "
import os
files = [f for f in os.listdir('.') if f.endswith('.py')]
print(f'Found {len(files)} Python files')
" --json
```

#### Pattern B: Safe Copy-on-Write Staging
Mount source files as read-only (`--ro`), stage a copy into the writable work folder (`--copy`), and freely modify the copy:
```bash
cocoon run \
  --ro ./configs \
  --copy ./configs/app.json \
  --work ./sandbox_work \
  --runtime python \
  -c "import json; d = json.load(open('app.json')); d['debug'] = True; json.dump(d, open('app.json', 'w'))" \
  --json
```
*(The original `./configs/app.json` remains completely untouched).*

#### Pattern C: Creating & Manipulating Output Files
Provide an explicit `--work` directory to write generated code, artifacts, or logs:
```bash
cocoon run --work ./agent_output --runtime node -c "
const fs = require('fs');
fs.writeFileSync('result.json', JSON.stringify({ status: 'completed', timestamp: Date.now() }));
" --json
```

#### Pattern D: Installing and Using Packages in the Sandbox
Packages can be installed inside `--work` without affecting the host environment:
* **Python:**
  ```bash
  cocoon run --work ./py_env --runtime python -c "
  import subprocess, sys
  subprocess.run([sys.executable, '-m', 'pip', 'install', '--target', '.', 'pyyaml'], check=True)
  import yaml
  print('YAML loaded successfully')
  " --json
  ```
* **Node.js / Bun:**
  ```bash
  cocoon run --work ./js_env --runtime bun -c "
  require('child_process').execSync('bun add lodash');
  const _ = require('lodash');
  console.log(_.chunk([1, 2, 3, 4], 2));
  " --json
  ```

---

### 3. Polyglot Multi-Language Pipelines (`cocoon pipeline`)

When an agent needs to use multiple programming languages together in a single workflow, use `cocoon pipeline`. All steps run sequentially in the **exact same virtual workspace**:

```bash
cocoon pipeline \
  -s "python:import json; json.dump({'data': [10, 20, 30]}, open('shared.json', 'w'))" \
  -s "bun:const fs = require('fs'); const d = JSON.parse(fs.readFileSync('shared.json')); d.total = d.data.reduce((a,b)=>a+b); fs.writeFileSync('shared.json', JSON.stringify(d));" \
  -s "pwsh:Get-Content shared.json" \
  --json
```

**Pipeline JSON Output:**
```json
{
  "success": true,
  "total_duration_ms": 285.4,
  "steps": [
    { "step": 1, "runtime": "python", "exit_code": 0, "stdout": "", "duration_ms": 190.2, "timed_out": false },
    { "step": 2, "runtime": "bun", "exit_code": 0, "stdout": "", "duration_ms": 18.5, "timed_out": false },
    { "step": 3, "runtime": "pwsh", "exit_code": 0, "stdout": "{\"data\":[10,20,30],\"total\":60}", "duration_ms": 76.7, "timed_out": false }
  ]
}
```

---

### 4. Running Any Arbitrary Compiler or Toolchain
To run compilers or tools without a dedicated `-r` preset, pass the command after `--`:

```bash
# Compile and run a C program in the sandbox
cocoon run --work ./build -- clang main.c -o app.exe && ./app.exe

# Run .NET / C#
cocoon run --work ./dotnet_project -- dotnet run
```

---

### 5. Checking Available Runtimes
To inspect which runtimes are available on the current host:
```bash
cocoon info
```
Supported presets: `python`, `node`, `bun`, `deno`, `go`, `java`, `rust` / `cargo`, `pwsh` / `powershell`, `bash` / `sh`, `ruby`, `php`, `lua`.

---

## 🔒 Built-in Sandbox Invariants (Enforced by Default)

1. **Read-Only by Default (`--ro .`):**
   * The current working directory is automatically mounted as **strictly read-only** by default.
   * Any attempt by scripts or code to write, append, or delete host files is physically blocked at the OS kernel level (`PermissionError: Permission denied`).

2. **Zero Host Writes (Strict Workdir Isolation):**
   * Code can **never** get write access to anything other than the designated `--work` folder (or the auto-cleaned ephemeral scratchpad).
   * To edit, refactor, or test modifications on existing code, use `--copy <path>` to stage a copy into the work folder and manipulate it there safely.

3. **30-Second Execution Timeout by Default:**
   * Every execution is automatically capped at **30 seconds** by default (override with `--timeout <secs>`). Infinite loops or hangs will auto-terminate without freezing the agent.

4. **Verify Telemetry in `--json`:**
   * Always verify `result.exit_code === 0` and `result.timed_out === false` before consuming `result.stdout`.
