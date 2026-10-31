# Cocoon Security & Vulnerability Remediation Guide

> **Audience:** AI Agent developers, platform engineers, and security auditors.  
> **Status:** Production-hardened (v0.2.0+).  
> **Scope:** Threat model, vulnerability catalog, platform mitigations (Windows & Linux), and agent-safe integration patterns.

---

## 1. Threat Model Overview

When AI agents execute untrusted or model-generated code, they introduce attack vectors fundamentally distinct from traditional software services. A model susceptible to prompt injection, hallucination, or malicious tool-use can generate code that attempts:

1. **Credential Exfiltration:** Reading tokens from `~/.aws/credentials`, `~/.ssh/id_rsa`, `~/.git-credentials`, `%APPDATA%`, or environment variables, and posting them via raw TCP/UDP sockets or HTTP requests.
2. **Local IPC & Service Hijacking:** Connecting to local daemon Named Pipes on Windows (`\\.\pipe\docker_engine`, `\\.\pipe\openssh-ssh-agent`) or abstract UNIX domain sockets on Linux to escape the container boundary.
3. **Desktop Session Snooping:** Reading the host clipboard (`GetClipboardData`), poisoning the clipboard, sending synthetic keystrokes, or intercepting window handles (Shatter attacks).
4. **Filesystem Breakouts:** Creating symlinks, NTFS directory junctions, or hardlinks pointing to root drives or host sensitive files.
5. **Denial-of-Service (DoS):** Launching fork bombs, spinning infinite CPU loops, writing infinite files (disk exhaustion), or generating gigabytes of stdout to crash the agent chassis memory.
6. **Host Dependency & Toolchain Tampering:** Installing arbitrary packages globally, polluting host cache directories, or planting malicious DLLs in the working directory.

Cocoon employs a **two-tier defense-in-depth model** to neutralize these attacks:
- **Tier 1 (Hermetic Interpreters):** Language presets invoke hermetic flags (`-I -S -B`, `--disable-gems`, `-n`, `--no-addons`) and sanitize environment variables.
- **Tier 2 (Kernel Sandbox Drivers):** Platform drivers enforce OS-level controls (Windows Job Objects & UI Restrictions, Linux bubblewrap namespaces, storage quotas, and atomic process tree termination).

---

## 2. Vulnerability & Remediation Matrix

| Threat / Attack Vector | Severity | Platform Reality & Nuance | Cocoon Production Mitigation |
| :--- | :---: | :--- | :--- |
| **Outbound Sockets / Exfiltration** | **Critical** | **Platform Difference:** Linux enforces kernel-level network namespace isolation (`CLONE_NEWNET`); Windows enforces **cooperative proxy sinkholing**. | **Linux:** `bwrap --unshare-all` severs all network interfaces and raw sockets.<br>**Windows:** Proxies (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`) sink to `127.0.0.1:0`. Blocks standard HTTP/HTTPS clients (`requests`, `urllib`, `fetch`, `curl`, `pip`). *Notice:* Raw `socket.connect()` or proxy-ignoring clients on Windows can reach the network without external WFP/firewall rules. |
| **Host Credential Harvesting** | **Critical** | **Ambient Filesystem Access on Windows:** Windows has no unprivileged mount namespaces. Scripts run with the ambient read permissions of the launching user outside `--ro` mounts. | Synthetic Dummy User Profile created in ephemeral scratchpad redirects `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA` away from host secrets (`~/.aws`, `~/.ssh`). Host folders mounted with `--ro` receive NTFS Deny ACEs. |
| **Named Pipe / IPC Hijacking** | **Critical** | Child could connect to `\\.\pipe\docker_engine` or `ssh-agent` | Explicitly strips `DOCKER_HOST`, `SSH_AUTH_SOCK`, `WSL_INTEROP`, and cloud tokens even if `inherit_env` is requested. |
| **Desktop Clipboard Theft** | **High** | Script could read clipboard via `GetClipboardData` / PowerShell | Enforces Win32 Job Object UI Restrictions: `JOB_OBJECT_UILIMIT_READCLIPBOARD` & `WRITECLIPBOARD`. |
| **Desktop / Window Hijacking** | **High** | Script could inject synthetic keystrokes or hook window handles | Enforces `JOB_OBJECT_UILIMIT_HANDLES`, `GLOBALATOMS`, `SYSTEMPARAMETERS`, and `DESKTOP`. |
| **Fork Bombs & Handle Exhaustion** | **High** | Unlimited processes could exhaust the OS PID table | Hard active process limit (`max_processes: 32` default) enforced via Windows Job Object and Linux `RLIMIT_NPROC`. |
| **CPU Starvation / Infinite Loops** | **High** | Tight loops consumed 100% host CPU | Hard execution timeout (30s default) with atomic tree termination (`TerminateJobObject` / `kill`), plus CPU rate throttling. |
| **Storage / Disk Bomb DoS** | **High** | **Teardown Check on Windows:** Disk writes hit disk during execution and are verified upon process exit. | Linux enforces hard kernel cap via `RLIMIT_FSIZE`. Windows verifies total scratchpad disk quota (`disk_limit_mb: 512MB` default) upon process exit, aborting before artifacts are accepted. |
| **Stdout Buffer Memory Exhaustion** | **High** | Infinite print loop (`while true: print("A")`) exhausted agent RAM | Bounded 10 MB streaming buffers with chunked readers and truncation flags (`stdout_truncated` / `stderr_truncated`). |
| **Junction & Symlink Breakouts** | **High** | **Teardown Scan on Windows:** Junctions are created in-flight and detected upon exit. | Universal post-execution canonicalization check (`verify_no_breakouts`) scans the workspace upon exit, rejecting any escape pointing outside the workspace root with `SecurityViolation`. |
| **Workspace Permission Leakage** | **High** | Granting overly broad ACLs (`Everyone`) exposes workspace to other users | Workspace permissions maintain standard user inheritance. If nested inside a `--ro` mount, inheritance is broken and Full Control is granted strictly to the current user (`%USERNAME%`), NEVER `Everyone` (`*S-1-1-0`). |
| **DLL Preloading (CWD Hijacking)**| **Medium** | Windows loader searched workspace for DLLs before System32 | Injects `CWDIllegalInDllSearch=0x2` to disallow loading DLLs from current working directory. |
| **Linux Shared Memory Leakage** | **Medium** | Child process could access host `/dev/shm` IPC segments | Explicitly mounts a fresh, isolated `--tmpfs /dev/shm` per run in bubblewrap. |
| **Python Dependency Poisoning** | **Medium** | Scripts loaded host `site-packages` or wrote `.pyc` files | Default Python preset runs with `-I -S -B`. Dedicated `python-skill` preset provided for trusted document skills. |
| **Other Runtimes Auto-Install** | **Medium** | Node/Ruby/PHP/Bun loaded user configs or auto-fetched packages | Enforces `--no-addons` (Node), `--disable-gems` (Ruby), `-n` (PHP), `--no-prompt` (Deno), `--no-install` (Bun). |
| **Offline Toolchain Poisoning** | **Medium** | Toolchains fetched packages from npm/crates.io/go proxy | Injects `CARGO_NET_OFFLINE=true`, `GOPROXY=off`, `GONOSUMDB=*`, `npm_config_offline=true`, `PIP_NO_INDEX=1`. |

---

## 3. Deep-Dive: OS-Level Defensive Layers

### 3.1 Windows Job Object Hardening (`cocoon-platform`)

On Windows, Cocoon wraps every execution inside a native Win32 Job Object configured with three distinct limit classes:

```rust
// 1. Resource Limits (Active Processes, Memory, Breakaway Prevention)
let mut limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE 
    | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
    | JOB_OBJECT_LIMIT_ACTIVE_PROCESS; // Capped at 32

// 2. CPU Rate Control
cpu_info.ControlFlags = JOB_OBJECT_CPU_RATE_CONTROL_ENABLE 
    | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;

// 3. Basic UI Restrictions (Zero Desktop Access)
ui_restrictions.UIRestrictionsClass = JOB_OBJECT_UILIMIT_HANDLES
    | JOB_OBJECT_UILIMIT_READCLIPBOARD
    | JOB_OBJECT_UILIMIT_WRITECLIPBOARD
    | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
    | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
    | JOB_OBJECT_UILIMIT_GLOBALATOMS
    | JOB_OBJECT_UILIMIT_DESKTOP
    | JOB_OBJECT_UILIMIT_EXITWINDOWS;
```

#### Why `JOB_OBJECT_BASIC_UI_RESTRICTIONS` Matters
Standard Windows processes inherit the interactive user desktop (`WinSta0`). Without UI restrictions:
- Any Python script with `import ctypes` can read whatever the user currently has in their clipboard (passwords, 2FA tokens, private keys).
- Any process can broadcast `WM_SETTEXT` or `WM_KEYDOWN` to other windows running in the session.
Enforcing these restrictions severs the process from the interactive desktop completely.

### 3.2 Linux Bubblewrap & POSIX Isolation (`cocoon-platform`)

On Linux, Cocoon executes through Bubblewrap (`bwrap`) with unprivileged user namespaces:

```bash
bwrap \
  --unshare-all \          # Unshares mount, net, ipc, pid, uts, and cgroup
  --die-with-parent \      # Kills child immediately if supervisor dies
  --dev /dev \             # Minimal safe device nodes
  --proc /proc \           # Private virtual procfs (only sees itself as PID 1)
  --tmpfs /tmp \           # Ephemeral tmpfs (zero persistence)
  --tmpfs /dev/shm \       # Private POSIX shared memory (isolates host IPC)
  --ro-bind /usr /usr ...  # Mounts host binaries strictly read-only
```

In addition, Linux process limits are enforced at the syscall level via `libc::setrlimit` in `pre_exec`:
- `RLIMIT_AS`: Virtual address space memory limit.
- `RLIMIT_NPROC`: Max process count limit.
- `RLIMIT_FSIZE`: File size output quota.

### 3.3 Windows Security Boundaries & Platform Realities

Developers deploying Cocoon on bare-metal Windows must understand the fundamental architectural distinctions between Windows and Linux unprivileged isolation:

1. **Network Isolation: Cooperative Proxy Sinking vs. Kernel Namespaces**
   - **Linux:** Bubblewrap uses kernel user and network namespaces (`CLONE_NEWNET`). In this mode, no raw sockets, TCP packets, or UDP datagrams can reach external interfaces without explicit veth pairing.
   - **Windows:** Windows Job Objects do not provide network namespace virtualization. Cocoon enforces network isolation by nullifying standard proxy environment variables (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY` pointing to `http://127.0.0.1:0`) and injecting offline flags into language runtimes.
   - **The Reality:** Cooperative HTTP/HTTPS clients (such as Python `urllib`/`requests`, Node `fetch`, `npm`, `pip`, `curl`) fail to connect. However, **raw TCP sockets** (e.g. `socket.socket().connect(...)`) or clients explicitly ignoring proxy environment variables can still reach the network.
   - **Production Requirement:** If raw socket severance on Windows is required, Cocoon should be run inside a Linux container (Docker/WSL2) or behind host-level Windows Filtering Platform (WFP) / firewall rules.

2. **Filesystem Isolation: Explicit NTFS Deny ACEs vs. Mount Namespaces**
   - **Linux:** Bubblewrap constructs a pristine mount namespace where `/` is a read-only or tmpfs mount, and only explicitly requested folders are bind-mounted. Unmounted host paths (such as `/home/user/.ssh`) literally do not exist inside the namespace.
   - **Windows:** Windows has no native unprivileged mount namespace primitive. Cocoon implements read-only protection by injecting NTFS Deny ACEs (`DE,DC,WD,AD` - Delete, Delete Child, Write Data, Append Data) onto the specific paths passed to `--ro`.
   - **The Reality:** Deny ACEs protect the designated folders against writes and deletes, but do *not* deny read access. Crucially, ambient host directories outside `--ro` and `--work` (e.g., `C:\Users\<user>\...`) remain accessible for reading under the launching user's Windows access token. To mitigate credential theft, Cocoon synthesizes an isolated dummy user profile (`USERPROFILE`, `APPDATA`, `LOCALAPPDATA`) pointing into the ephemeral scratchpad.

3. **In-Flight Execution vs. Teardown Checks (Junctions & Quotas)**
   - On Windows, directory junction scans (`verify_no_breakouts`) and total scratchpad disk quota verifications (`verify_disk_quota`) execute **post-run upon process exit**.
   - If an untrusted script creates an NTFS junction or writes hundreds of megabytes during its execution slice, those operations physically occur on the host disk before Cocoon detects the violation upon exit and halts artifact propagation.

4. **Workspace Permissions Hardening**
   - Unlike legacy approaches that granted wide-open permissions (`Everyone` / `*S-1-1-0`), Cocoon strictly preserves standard inherited user permissions. If the workspace is nested inside a `--ro` mount, inheritance is broken and Full Control is granted strictly to the current executing user (`%USERNAME%`), with explicit revocation upon process teardown.

---

## 4. Filesystem Jail & Universal Escape Verification

### 4.1 Fail-Closed Read-Only Protection
On Windows, read-only mounts (`--ro`) apply a Deny ACE via `icacls`:
```powershell
icacls <dir> /deny *S-1-1-0:(OI)(CI)(DE,DC,WD,AD)
```
- **Fail-Closed Guarantee:** If `icacls` returns an error code, Cocoon **immediately aborts** execution with `CocoonError::SecurityViolation`. Under no circumstances will a process launch if filesystem read-only locks fail to land.
- **Automatic Restoration:** When execution finishes (or panics), `ReadOnlyGuard::drop()` removes the deny ACE and restores standard directory permissions.

### 4.2 Universal Breakout Verification
Attackers can attempt to bypass filesystem locks by creating:
1. Unix Symbolic Links (`ln -s /etc/passwd ./pass`)
2. Windows NTFS Directory Junctions (`mklink /J ./escape C:\Users`)
3. NTFS Hardlinks (`mklink /H ./link C:\secret.txt`)

Cocoon addresses this with `Scratchpad::verify_no_breakouts()`:
```rust
// Recursively scans every entry inside the workspace post-run:
if let Ok(canon) = path.canonicalize() {
    if !canon.starts_with(workspace_root) {
        return Err(CocoonError::SecurityViolation(format!(
            "Detected path escape pointing outside workspace: {:?} -> {:?}",
            path, canon
        )));
    }
}
```
If **any** file inside the workspace canonicalizes to an inode or target outside the designated workspace, the run is terminated with an error, and the staged changes are not propagated.

---

## 5. Hermetic Language Runtime Presets

Every language runtime supported by Cocoon is configured to prevent host configuration leakage and dynamic package loading:

```
┌─────────────────────────────────────────────────────────────┐
│ Cocoon Hermetic Interpreter Flag Matrix                     │
├─────────────────┬───────────────────────────────────────────┤
│ Runtime         │ Enforcement Flags & Arguments             │
├─────────────────┼───────────────────────────────────────────┤
│ Python (model)  │ python -I -S -B                           │
│ Python Skill    │ python -B (allows site-packages)          │
│ Node.js         │ node --no-addons                          │
│ Ruby            │ ruby --disable-gems                       │
│ PHP             │ php -n (ignores host php.ini)             │
│ Deno            │ deno eval/run --no-prompt                 │
│ Bun             │ bun -e --no-install                       │
│ PowerShell      │ pwsh -NoProfile -NonInteractive -Bypass   │
│ Bash            │ bash --noprofile --norc                   │
│ Rust            │ cargo script --offline                    │
│ Go              │ go run -mod=readonly                      │
└─────────────────┴───────────────────────────────────────────┘
```

### Environment Sanitization
Cocoon automatically injects these environment variables to neutralize dynamic package managers:
- `PIP_NO_INDEX=1` & `PYTHONNOUSERSITE=1` (Prevents pip/python user installations)
- `NODE_OPTIONS=""` (Neutralizes flag injection via parent environment)
- `RUBYOPT=""` & `PERL5OPT=""` (Neutralizes hook injection)
- `CARGO_NET_OFFLINE=true` (Prevents Cargo from contacting crates.io)
- `GOPROXY=off` & `GONOSUMDB=*` (Prevents Go from downloading modules)
- `npm_config_offline=true` (Prevents npm from downloading packages)
- `CWDIllegalInDllSearch=0x2` (Disables Windows CWD DLL search order)

---

## 6. Audit Logging & Structured Telemetry

For integration with Security Information and Event Management (SIEM) systems and Agent Supervisors, Cocoon outputs structured JSON telemetry:

### 6.1 CLI Logging Flag: `--log-json`
```bash
cocoon --log-json run -r python -c "print('Audit verified')"
```
Emits NDJSON (newline-delimited JSON) tracing logs with event classification:
```json
{
  "timestamp": "2026-10-31T08:35:12.102Z",
  "level": "WARN",
  "target": "cocoon::security",
  "event": "execution_timeout",
  "executable": "C:\\Program Files\\Python311\\python.exe",
  "timeout_ms": 30000,
  "message": "Process exceeded execution timeout limit; terminating entire job object tree"
}
```

### 6.2 Agent JSON Telemetry Schema
When invoked with `--json`, Cocoon returns machine-readable execution summaries:
```json
{
  "exit_code": 0,
  "stdout": "Success\n",
  "stderr": "",
  "duration_ms": 42.15,
  "timed_out": false,
  "oom_killed": false,
  "termination_reason": null,
  "stdout_truncated": false,
  "stderr_truncated": false,
  "peak_memory_mb": 18
}
```

#### Termination Reason Values
| Value | Description |
| :--- | :--- |
| `null` | Process completed normally within all boundaries. |
| `"timeout"` | Process exceeded execution timeout (terminated via atomic Job Object / SIGKILL). |
| `"memory_exceeded"` | Process breached RAM limit or returned `0xC0000017` (`STATUS_NO_MEMORY`). |
| `"process_count_exceeded"` | Script attempted to exceed max active process limit (fork bomb). |
| `"security_violation"` | Failed read-only lock, junction escape detected, or quota breached. |

---

## 7. Integration Recipes for AI Agents

### Pattern 1: Safe Model-Generated Python Execution
```python
import subprocess
import json

def run_untrusted_python(code: str) -> dict:
    cmd = [
        "cocoon", "run",
        "--runtime", "python",     # Uses python -I -S -B
        "--timeout", "10",         # 10s timeout
        "--mem", "128",            # 128 MB RAM cap
        "--disk", "64",            # 64 MB write quota
        "--max-procs", "4",        # Max 4 processes
        "--json",                  # Structured telemetry
        "-c", code
    ]
    res = subprocess.run(cmd, capture_output=True, text=True)
    return json.loads(res.stdout)
```

### Pattern 2: Safe Document Processing with Skills
```python
def process_excel_sheet(sheet_path: str, script: str) -> dict:
    cmd = [
        "cocoon", "run",
        "--runtime", "python-skill",   # Allows openpyxl, pandas, docx
        "--ro", sheet_path,            # Sheet is strictly read-only
        "--json",
        "-c", script
    ]
    res = subprocess.run(cmd, capture_output=True, text=True)
    return json.loads(res.stdout)
```

### Pattern 3: Atomic Polyglot Pipeline
```bash
cocoon pipeline \
  --step "node:const fs = require('fs'); fs.writeFileSync('data.txt', 'hello');" \
  --step "python:with open('data.txt') as f: print(f.read().upper())" \
  --json
```
Both steps share the exact same ephemeral workspace. Staged files are isolated and destroyed upon exit.

---

## 8. Security Invariants (Automated Regression Test Suite)

Cocoon maintains an automated regression test suite in [`crates/cocoon-cli/tests/regression_tests.rs`](file:///c:/space/projects/cocoon/crates/cocoon-cli/tests/regression_tests.rs) that validates every invariant before release:

1. `regression_default_network_is_denied`: Proves default network access is strictly false.
2. `regression_default_max_processes_is_enforced`: Proves default max active processes is 32.
3. `regression_default_disk_limit_is_enforced`: Proves storage quota default is 512 MB.
4. `regression_default_timeout_is_enforced`: Proves 30-second default execution timeout.
5. `regression_telemetry_json_schema_matches_agent_spec`: Asserts backwards-compatibility of the agent JSON output schema.
6. `regression_mount_rule_immutability`: Asserts read-only mount descriptors cannot be corrupted.
7. `regression_scratchpad_cleanup_on_drop`: Asserts ephemeral directories are unlinked when dropped.
8. `regression_clean_scratchpad_passes_breakout_verification`: Asserts universal canonicalization breakout validation.
9. `regression_rust_preset_offline`: Asserts `cargo --offline` invariant.
10. `regression_go_preset_offline`: Asserts `go run -mod=readonly` invariant.
11. `regression_preset_runtime_resolution`: Asserts skipping of 0-byte Windows Store execution alias stubs.
12. `regression_default_cpu_limit_is_95_percent`: Asserts that default CPU usage is capped at 95% MAX.
13. `regression_default_gpu_allowed_is_true`: Asserts that GPU hardware access defaults to true subject to allocator safety guards.
14. `regression_memory_cap_enforces_95_percent_ceiling`: Mathematically proves that requested or default memory never exceeds 95% of host physical RAM.

---

## 9. Hardware-Aware Resource Caps & Host Stability Guarantees

When AI agents run heavy computational workloads—such as local Small Language Models (SLMs), LoRA fine-tuning, audio transcription, or multi-gigabyte pandas operations—unbounded sandboxes can freeze the host operating system or trigger GPU driver panics. Cocoon enforces a **hardware-aware 95% maximum ceiling** and kernel-level scheduler protections across Windows, Linux, and macOS:

### 1. Host Physical Memory Auto-Awareness (The 95% Hard Ceiling)
- **Problem**: If an agent executes a memory-hungry script without specifying `--mem`, or specifies a memory limit exceeding physical hardware, the host OS enters severe paging thrashing or triggers a kernel panic/blue screen.
- **Cocoon Mitigation**:
  - Windows: Queries physical RAM via `GlobalMemoryStatusEx`.
  - Linux: Queries physical pages and page size via `sysconf(_SC_PHYS_PAGES)` and `sysconf(_SC_PAGESIZE)`.
  - **Enforcement**: Cocoon computes `max_safe_mb = (host_phys_mb * 95) / 100`. The effective sandbox memory ceiling is calculated as:
    $$\text{Effective Limit} = \min(\text{Requested Limit},\, 0.95 \times \text{Host Physical RAM})$$
  - Even if no `--mem` flag is passed, Cocoon **automatically** bounds the sandbox to 95% of the machine's physical RAM, mathematically guaranteeing that the host OS and critical system daemons retain reserved memory.

### 2. GPU VRAM Exhaustion & Multi-Vendor Driver Isolation
- **Problem**: Host OS Job Objects and standard process limits only control CPU system RAM. An unconstrained GPU allocation will allocate 100% of VRAM, crashing the Windows Desktop Window Manager (`dwm.exe`) or causing Linux GPU kernel TDR (Timeout Detection and Recovery) resets.
- **Cocoon Mitigation**:
  - **Complete GPU Detachment (`--no-gpu`)**: When `--no-gpu` is specified, Cocoon strips and masks all hardware driver interfaces across all major vendors:
    - NVIDIA: `CUDA_VISIBLE_DEVICES=""`, `NVIDIA_VISIBLE_DEVICES="void"`
    - AMD ROCm/HIP: `ROCR_VISIBLE_DEVICES=""`, `HIP_VISIBLE_DEVICES=""`
    - Intel: `ONEAPI_DEVICE_SELECTOR="none"`, `SYCL_DEVICE_FILTER="none"`
    - OpenCL & General: `OPENCL_DEVICE_NAME=""`, `GPU_DEVICE_ORDINAL=""`
  - **Safe SLM / Training Execution**: When GPU access is permitted, Cocoon automatically injects VRAM allocator fragmentation controls:
    ```bash
    PYTORCH_CUDA_ALLOC_CONF="max_split_size_mb:128"
    ```
    This prevents PyTorch and Ollama/vLLM backends from creating fragmented memory blocks that lock up system drivers.

### 3. CPU Priority Elevation & Host Scheduler Hardening
- **Problem**: Malicious or buggy guest code can call `SetPriorityClass(REALTIME_PRIORITY_CLASS)` or `sched_setscheduler(SCHED_FIFO)`, raising thread priority above host audio drivers, display managers, and keyboard/mouse handlers, causing a permanent system freeze.
- **Cocoon Mitigation**:
  - Windows: Sets `JOB_OBJECT_LIMIT_PRIORITY_CLASS` with `NORMAL_PRIORITY_CLASS`. Windows Job Objects enforce this as an unbypassable kernel constraint—any attempt by child processes to elevate priority to Realtime or High is rejected by the kernel.
  - Linux: In `pre_exec`, Cocoon applies `setrlimit(RLIMIT_RTPRIO, 0)`, preventing any process from acquiring real-time scheduling priority.
  - CPU Rate Cap: Windows Job Object CPU rate control is capped at 95% MAX (`(cpu_pct.clamp(1, 95)) * 100`).

### 4. Crash Core Dump Snooping & Storage Flooding
- **Problem**: When large models or intensive workloads crash, default OS handlers generate multi-gigabyte core dumps (`core.<pid>`) or Windows Error Reporting (WER) minidumps. This rapidly exhausts disk space and writes confidential weights and prompts into world-readable crash files.
- **Cocoon Mitigation**:
  - Linux: In `pre_exec`, Cocoon applies `setrlimit(RLIMIT_CORE, 0)`. Core dump generation is completely disabled.
  - Windows: Configured with `JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION`, and suppresses debugger attachment dialogs (`COMPlus_DbgJitDebugLaunchSetting=0`, `DOTNET_DbgJitDebugLaunchSetting=0`).

---

## 10. Chassis Architecture vs. Model Agency: Never Let the Agent Pick Its Own Sandbox Flags

A critical design rule for building secure AI agent platforms is **separation of policy from model agency**:

> [!CAUTION]
> **Never expose sandbox execution flags (`--allow-network`, `-r python-skill`, `--no-gpu`, `--work`, `--ro`) as arguments in the tool schema exposed to the LLM.**

If an LLM or agent tool-calling schema allows the model to pass sandbox flags:
1. A prompt injection (e.g., hidden in a retrieved document, email, or database record) can instruct the LLM to call `execute_code(code=..., allow_network=True)`.
2. The model could intentionally switch from `-r python` (which runs in isolated standard library mode `-I -S -B`) to `-r python-skill`, bypassing hermetic standard-library isolation.
3. The model could point `--work` to arbitrary host paths outside the session.

### Recommended Chassis Pattern

The outer agent chassis / tool harness (written in Python, TypeScript, Go, etc.) must hardcode the sandbox boundaries based on deterministic application policy:

| Workload Type | Preset Selected by Chassis | Network Flag | Mount Policy |
| :--- | :--- | :--- | :--- |
| **Untrusted Model-Generated Code** | `-r python` (isolated `-I -S -B`) | Default-Deny (No `--allow-network`) | `--ro <repo>` (Read-only worktree) |
| **Document Skills (Excel, Word, PDF)** | `-r python-skill` | Default-Deny (No `--allow-network`) | `--ro <input_dir> --work <session_worktree>` |
| **Web Scraping / API Tools** | Dedicated scraper tool | `--allow-network` hardcoded by host | Ephemeral workspace only |

The tool definition exposed to the LLM should look strictly like this:
```json
{
  "name": "run_python_code",
  "description": "Executes Python code in an isolated sandbox. Only standard library is available.",
  "parameters": {
    "type": "object",
    "properties": {
      "code": {
        "type": "string",
        "description": "The Python source code to execute."
      }
    },
    "required": ["code"]
  }
}
```

The chassis tool implementation constructs the command deterministically:
```python
def handle_run_python_code(code: str, session_id: str) -> dict:
    # All security flags are picked by the CHASSIS, NOT the MODEL
    cmd = [
        "cocoon", "run",
        "--runtime", "python",
        "--ro", get_session_worktree(session_id),
        "--work", get_session_output_dir(session_id),
        "--timeout", "30",
        "--mem", "512",
        "--disk", "256",
        "--json",
        "-c", code
    ]
    res = subprocess.run(cmd, capture_output=True, text=True)
    return json.loads(res.stdout)
```



