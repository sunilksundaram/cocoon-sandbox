# Why AI Agents Keep Breaking Local Systems (And How Cocoon Fixes It in < 5ms)

### Running untrusted LLM-generated code across Python, Node.js, Bun, Go, and Rust without containers, VMs, or file corruption.

---

![Cocoon Hero Banner](https://raw.githubusercontent.com/sunilksundaram/cocoon/main/docs/assets/banner.png)
*(Image placeholder: A lightweight, crystalline cocoon shielding a system kernel from dynamic execution waves)*

---

If you have built an autonomous coding assistant, a data analysis agent, or an interactive LLM reasoning loop in the past two years, you know the dread of this function signature:

```python
def run_code(code: str) -> str:
    # Danger zone
    return subprocess.check_output(["python", "-c", code])
```

Autonomous agents do not simply answer questions anymore. **They write code, test hypotheses, manipulate datasets, install libraries, and refactor files.**

Yet, giving an LLM terminal access on your workstation or production server is an engineering nightmare:
- **Catastrophic Host Mutation:** A subtle hallucination like `os.system("rm -rf data /")` or `shutil.rmtree(os.path.dirname('.'))` can instantly wipe the developer's workspace or system root.
- **Cache & Disk Pollution:** Every Python invocation drops `__pycache__` folders and `.pyc` files into source trees; `npm` or `pip` clutter the developer's global package paths.
- **Infinite Loops & Memory Spikes:** An LLM writing an un-converged `while True:` loop or allocating an uncapped multi-gigabyte numpy array freezes the host operating system.

Until today, developers were forced to pick between two flawed extremes: **reckless raw execution** or **sluggish, heavyweight containers**.

Enter **[Cocoon](https://github.com/sunilksundaram/cocoon)** (`cocoon-sandbox`) — a featherweight systems engine written in Rust that provides sub-millisecond, cross-platform isolation, virtual read-only projections, and polyglot execution for 12+ programming runtimes.

---

## 1. The Real Problem: The Container Tax vs. The Security Void

When modern engineering teams attempt to sandbox AI-generated code, they usually consider three architectures — and each fails in modern agent workflows:

```
┌────────────────────────────────────────────────────────────────────────┐
│                        THE SANDBOX TRILEMMA                            │
├────────────────────────────┬─────────────────────────────┬─────────────┤
│ Approach                   │ Startup Overhead            │ Safety / UX │
├────────────────────────────┼─────────────────────────────┼─────────────┤
│ 1. Raw `subprocess`        │ < 5 ms                      │ 🔴 Zero     │
│ 2. Docker / Containers     │ ~1,200 ms - 3,500 ms        │ 🟡 Rigid    │
│ 3. MicroVMs (Firecracker)  │ ~150 ms - 400 ms + Infra VM │ 🟡 Complex  │
│ 4. Cocoon Sandbox          │ < 3 ms                      │ 🟢 Ironclad │
└────────────────────────────┴─────────────────────────────┴─────────────┘
```

1. **Docker is too slow for interactive reasoning loops.** When an agent is in a 10-turn ReAct reasoning loop, spinning up a Docker container on every turn introduces a 2 to 4 second penalty per step. Over 10 turns, the user waits 30+ seconds just for Docker daemon orchestration.
2. **Docker doesn't run natively inside local dev environments.** Requiring developer workstations to have Docker Desktop running (with its memory overhead and license requirements) breaks local developer tooling, IDE plugins, and command-line agents.
3. **WASM runtimes are language-restricted.** WebAssembly engines (Wasmtime, Wasmer) are great for pure algorithms, but they cannot run native Python C-extensions (like NumPy, Pandas, PyTorch), cannot invoke Node.js packages, and cannot run shell toolchains.

Agents need something different: **the native speed of the host toolchain, but wrapped in a kernel-enforced, read-only straitjacket.**

---

## 2. What is Cocoon?

**Cocoon** is an open-source, native sandbox engine built in Rust. It does not run a guest operating system. Instead, it leverages the operating system’s **native kernel-level isolation primitives** to jail processes in under 5 milliseconds:

- **On Windows:** Uses **Windows Job Objects** (hard memory caps and process-tree termination) combined with **Inherited NTFS Deny ACLs**.
- **On Linux:** Uses **Unprivileged User Namespaces**, read-only bind mounts (`MS_RDONLY`), `tmpfs` scratchpads, and optional **Bubblewrap** / Landlock LSM.
- **On macOS:** Uses the Apple **Seatbelt Kernel Framework** (`libsandbox` via dynamic SBPL profiles) to enforce hardware-grade read-only barriers.

```
                    ┌────────────────────────────┐
                    │    AI Agent / Host App     │
                    └─────────────┬──────────────┘
                                  │ (CLI / Rust / JSON / Python)
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

## 3. The Core Built-In Invariants

Cocoon is built around non-negotiable guarantees designed specifically for autonomous code generation:

### Invariant 1: Read-Only Host by Default (`--ro .`)
When an agent runs in a directory, that directory is mounted as **strictly read-only**. If the agent generates code that attempts `open("important_data.csv", "w")` or `os.remove("main.py")`, the OS kernel physically blocks the system call with `Permission denied`. No files on the host can be overwritten or deleted.

### Invariant 2: Default-Deny Network Access (`allow_network: false`)
A write ban alone does not protect you if an untrusted script can read a credential file and send it out over a socket. Cocoon completely disables network interfaces (Linux) and nullifies proxy endpoints (Windows) by default. Scripts cannot make outbound HTTP requests, connect to local chassis daemons, or probe cloud metadata (`169.254.169.254`) unless `--allow-network` is explicitly passed.

### Invariant 3: Ephemeral Copy-on-Write (`--copy` and `--work`)
If the agent needs to edit existing files (for example, refactoring code or tweaking a configuration file), you stage a copy:
```bash
cocoon run --ro ./repo --copy ./repo/app.json --work ./agent_output \
  -r python -c "import json; d = json.load(open('app.json')); d['debug']=True; json.dump(d, open('app.json','w'))"
```
The agent freely mutates `app.json` inside its private `--work` directory. The original file in `./repo` remains completely untouched.

### Invariant 4: Hard Timeouts, Process Caps & Storage Quotas
- **30-Second Timeout:** Monotonic timer triggers atomic process tree termination (`TerminateJobObject` / `kill`).
- **Process Cap (32 active):** Enforced via Win32 Job Object and Linux `RLIMIT_NPROC` to eliminate fork bombs.
- **Disk Write Quota (512 MB):** Post-run verification and kernel limits prevent scripts from filling the host drive.
- **Output Buffers (10 MB):** Prevents runaway print loops from exhausting agent host memory.

### Invariant 5: Desktop & Clipboard Isolation
Standard user scripts can quietly read your clipboard (`GetClipboardData`) to steal passwords and tokens. Cocoon enforces Win32 Job Object UI restrictions (`JobObjectBasicUIRestrictions`) blocking access to the host clipboard, user handles, and desktop messaging.

### Invariant 6: Hermetic Runtimes & Dual-Mode Python
- **Untrusted Code:** Default `python` runs with `-I -S -B` (isolated standard library mode; third-party packages, user-sites, and bytecode writes are blocked).
- **Document Skills:** Trusted format tools run via `python-skill` (loads `openpyxl`, `python-docx`, `pandas`).
- **Other Runtimes:** Node runs with `--no-addons` and neutralizes `NODE_OPTIONS`; Ruby runs with `--disable-gems`; PHP runs with `-n` (ignores `php.ini`). Offline toolchain flags are injected automatically.

### Invariant 7: Hardware-Aware 95% Ceiling & Real-Time Scheduler Lockout
Unbounded execution of machine learning models or multi-threaded loops can starve the host OS scheduler or exhaust physical RAM. Cocoon auto-detects host physical memory and CPU cores via native OS APIs (`GlobalMemoryStatusEx` on Windows, `sysconf` on Linux), automatically capping memory usage to **95% of host physical RAM** and clamping CPU rates. Furthermore, process priorities are locked to `NORMAL_PRIORITY_CLASS` on Windows and `RLIMIT_RTPRIO=0` on Linux, making it mathematically impossible for untrusted code to elevate to Realtime priority and freeze host input/display drivers.

### Invariant 8: GPU VRAM Isolation & Allocator Protection (`--no-gpu` vs. SLM Inference)
Win32 Job Objects and Linux cgroups do not govern GPU VRAM. An untrusted script allocating 100% of GPU memory can crash the Windows Desktop Window Manager (`dwm.exe`) or cause a GPU driver reset. Cocoon provides complete multi-vendor detachment (`--no-gpu`) across NVIDIA, AMD ROCm/HIP, Intel, and OpenCL. When GPU acceleration is permitted for local Small Language Models (SLMs like Phi-3 or Llama-3), Cocoon automatically injects PyTorch VRAM allocator fragmentation guards (`max_split_size_mb:128`) and suppresses crash core dumps (`RLIMIT_CORE=0`).

---

## 4. The Killer Feature: Polyglot Multi-Language Pipelines

One of Cocoon's most unique capabilities is the **`cocoon pipeline`** engine. 

Real-world AI agents frequently need to chain together different tools: a Python script extracts data from a dataframe, a Node.js script formats it for a React schema, Bun validates it, and PowerShell or Bash packages the output.

Previously, sharing state across different language processes safely required complex disk mounts or intermediate servers. With Cocoon, multiple languages execute sequentially inside the **exact same virtual workspace**:

```bash
cocoon pipeline \
  -s "python:import json; json.dump({'source':'Python','count':42}, open('state.json','w'))" \
  -s "node:const fs = require('fs'); const d = JSON.parse(fs.readFileSync('state.json')); d.count *= 2; fs.writeFileSync('state.json', JSON.stringify(d));" \
  -s "bun:const d = await Bun.file('state.json').json(); console.log('Bun verified final count:', d.count);" \
  --json
```

**What happens behind the scenes:**
1. A temporary, isolated scratchpad is initialized.
2. Step 1 (Python) writes `state.json` inside the sandbox.
3. Step 2 (Node.js) reads `state.json`, doubles the value, and rewrites it.
4. Step 3 (Bun) verifies and prints the result.
5. The entire execution returns machine-readable JSON telemetry.
6. The scratchpad is instantly destroyed on exit without leaving a single temporary file on the host.

---

## 5. Real-World Use Cases

### 1. Autonomous Coding Agents (Claude Desktop, Cursor, Antigravity, AutoGen)
Agents running in your IDE should never have direct `exec` power on your live project files. By wrapping the tool executor with Cocoon:
```python
# Minimal Python tool wrapper
import json, subprocess

def run_sandboxed(code: str, runtime: str = "python") -> dict:
    proc = subprocess.run(
        ["cocoon", "run", "-r", runtime, "-c", code, "--json"],
        capture_output=True,
        text=True
    )
    return json.loads(proc.stdout)
```
The agent can inspect files, run tests, and generate proof-of-concept modifications in `--work` without any chance of wrecking the user's uncommitted git changes.

### 2. Multi-Tenant LLM Backend APIs & SaaS
If you run an AI web service that allows users to "Chat with their CSV / Excel / Database", you cannot afford to spin up a Docker container for every user query. 

With Cocoon:
- Startup time drops from **2,500 ms to 3 ms**.
- Memory footprint is near-zero (no container daemon or guest OS RAM reservation).
- The user's uploaded dataset is mounted `--ro` so scripts can read and compute statistics without mutating or leaking data across tenants.

### 3. Automated Benchmark & Pull Request Evaluation
Running untrusted open-source Pull Requests or dynamic coding benchmark suites (like SWE-bench or HumanEval):
- Cocoon enforces strict CPU/RAM caps and kills runaway scripts.
- Bytecode caches (`__pycache__`) and package directories are redirected to in-memory paths, ensuring zero test pollution between benchmark iterations.

### 4. Safe Package Experimentation
Want an agent to try a third-party package without installing it into your machine's global environment?
```bash
cocoon run --work ./agent_env -r python -c "
import subprocess, sys
subprocess.run([sys.executable, '-m', 'pip', 'install', '--target', '.', 'scikit-learn'], check=True)
import sklearn
print('Isolated scikit-learn ready!')
"
```
The package installs inside `./agent_env` and is completely quarantined from the host Python installation.

### 5. High-Throughput Big Data, Document Conversions & Multimedia
What happens when an agent needs to extract data from a 500 MB Excel ledger, convert a 400-page PDF to Word, or transcode a video?

In Docker or virtual machines, mounting multi-gigabyte files incurs severe **VirtIO-FS virtualization penalties**, throttling I/O to 300–600 MB/s. 

Because Cocoon uses **native OS filesystem projections** (inherited Deny ACEs on Windows and bind mounts in Linux mount namespaces), reading from `--ro` happens at **raw NVMe SSD bus speeds (3,000–7,000 MB/s)** with zero copying:

```bash
# 1. Processing a massive 500MB Excel ledger with Pandas & Calamine at NVMe speed
cocoon run \
  --ro ./data_lake \
  --work ./output_dir \
  --timeout 180 \
  --disk 2048 \
  -r python-skill \
  -c "
import pandas as pd
df = pd.read_excel('data_lake/annual_ledger_500MB.xlsx', engine='calamine')
high_val = df[df['amount'] > 50000]
high_val.to_parquet('output_dir/filtered_ledger.parquet')
print(f'Extracted {len(high_val):,} transactions from {len(df):,} total rows.')
" --json

# 2. Converting a 400-page PDF to Word docx
cocoon run \
  --ro ./contracts \
  --work ./word_docs \
  --timeout 240 \
  -r python-skill \
  -c "
from pypdf import PdfReader
from docx import Document
reader = PdfReader('contracts/annual_filing_400_pages.pdf')
doc = Document()
for page in reader.pages:
    txt = page.extract_text()
    if txt: doc.add_paragraph(txt)
doc.save('word_docs/annual_filing.docx')
print(f'Converted {len(reader.pages)} PDF pages to Word docx.')
" --json

# 3. Audio & Video processing with ffmpeg
cocoon run --ro ./media --work ./audio --timeout 300 --disk 4096 -- \
  ffmpeg -i media/keynote_4k.mp4 -vn -acodec libmp3lame -q:a 2 audio/keynote_audio.mp3
```

---

## 6. How Cocoon Compares

| Feature | Raw Subprocess | Docker | WebAssembly (WASM) | Cocoon Sandbox |
| :--- | :--- | :--- | :--- | :--- |
| **Startup Latency** | **< 2ms** | 1,500–4,000ms | < 10ms | **< 3ms** |
| **Host Protection** | ❌ None | ✅ High | ✅ High | ✅ **Kernel-Enforced** |
| **Large File I/O Throughput (5GB file)** | 3,000–7,000 MB/s | ⚠️ 300–600 MB/s (VirtIO-FS) | ❌ Restricted | ✅ **3,000–7,000 MB/s (NVMe)** |
| **Default Network Security** | ❌ Full Access | ⚠️ Allowed by default | ❌ Restricted | ✅ **Default-Deny (`allow_network: false`)** |
| **Desktop & Clipboard Isolation** | ❌ None | ✅ Isolated (separate OS) | ✅ Isolated | ✅ **Win32 Job UI Restrictions** |
| **Hardware 95% Host Cap** | ❌ None | ⚠️ Static manual caps | ❌ Isolated | ✅ **Auto-Calculated 95% Ceiling** |
| **GPU Isolation & VRAM Guard** | ❌ Full Access | ⚠️ Requires nvidia-docker | ❌ None | ✅ **`--no-gpu` & Allocator Guard** |
| **C-Extension Support (NumPy, PyTorch)** | ✅ Full | ✅ Full | ❌ Very Limited | ✅ **Full** |
| **Multi-Language Runtimes** | ✅ Any installed | ⚠️ Requires massive images | ❌ Language-specific | ✅ **12+ Native Runtimes** |
| **Shared Multi-Language Pipeline** | ❌ Unsafe | ⚠️ Complex mounts | ❌ Difficult | ✅ **Built-in (`cocoon pipeline`)** |
| **Zero Docker Daemon Requirement** | ✅ Yes | ❌ Requires Daemon | ✅ Yes | ✅ **Single Native Binary** |

---

## 7. Getting Started

Cocoon is distributed as a single standalone executable and as native packages across both **PyPI** and **Crates.io**:

### Install via Python (Pip)
Pre-compiled native wheels are available for Windows, Linux, and macOS (no Rust toolchain required):
```bash
pip install cocoon-sandbox
```

### Install via Rust (Cargo)
```bash
cargo install cocoon-sandbox
```

### Quick Verification
Inspect the active OS isolation driver and all detected programming runtimes on your system:
```bash
cocoon info
```

Run your first sandboxed Python one-liner:
```bash
cocoon run --ro . -r python -c "import sys; print('Cocoon active on', sys.platform)"
```

---

## Conclusion: The Missing Layer in Agent Infrastructure

The AI agent revolution is shifting software engineering from static text generation to **dynamic execution**. 

If we want autonomous agents to be reliable, fast, and safe to use on everyday developer machines, we cannot rely on Docker containers that take seconds to start, nor can we allow unconstrained terminal access.

Cocoon fills this critical gap: **instant sub-millisecond execution, kernel-level read-only protection, and polyglot support for the modern multi-language stack.**

* Check out the code, architecture, and documentation on GitHub: **[https://github.com/sunilksundaram/cocoon](https://github.com/sunilksundaram/cocoon)**
* Star the project if you're building agents, and join the discussion on what runtimes and isolation primitives we should support next!

