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

*For complete security specifications, see the [Security & Vulnerability Remediation Guide](../../../docs/SECURITY_AND_VULNERABILITIES.md).*  
*For framework integration patterns (AutoGen, LangChain, LangGraph, LlamaIndex, Claude/MCP, Hermes), see the [User & Integration Guide](../../../docs/USER_GUIDE.md).*

---

## 🎯 When to Use Cocoon

Use `cocoon` whenever you need to:
1. **Safely execute untrusted code:** Run LLM-generated code or evaluate user scripts with default-deny network, read-only host mounts, and desktop/clipboard isolation.
2. **Inspect data read-only:** Mount repositories or datasets with `--ro` to guarantee that files cannot be modified, overwritten, or deleted.
3. **Experiment with files safely (Copy-on-Write):** Use `--copy <path>` to stage a copy of host files into a writable workspace and manipulate them without altering originals.
4. **Run document format skills:** Use `-r python-skill` to manipulate Excel (`openpyxl`), Word (`python-docx`), or datasets (`pandas`) without host mutation.
5. **Run multi-language pipelines:** Chain together steps in different languages (e.g. Python $\to$ Node $\to$ Bun $\to$ PowerShell) sharing the exact same virtual workspace.
6. **Enforce hard limits:** Guard against infinite loops, fork bombs, or memory leaks with `--timeout <secs>`, `--max-procs <count>`, `--disk <mb>`, and `--mem <mb>`.

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
  "oom_killed": false,
  "termination_reason": null,
  "stdout_truncated": false,
  "stderr_truncated": false,
  "peak_memory_mb": 14
}
```

*Note: If `stdout_truncated` is `true`, output exceeded the 10 MB buffer limit.*

---

### 2. Common Execution Patterns

#### Pattern A: Read-Only Repository Inspection (Untrusted Python)
Mount the codebase as strictly read-only (`--ro`). Untrusted code runs in isolated standard library mode (`-I -S -B`):
```bash
cocoon run --ro ./my_project --runtime python -c "
import os
files = [f for f in os.listdir('.') if f.endswith('.py')]
print(f'Found {len(files)} Python files')
" --json
```

#### Pattern B: Document Skills (`python-skill`)
When executing tools that require installed libraries (e.g., `openpyxl`, `python-docx`, `pandas`), use `-r python-skill`:
```bash
cocoon run --ro ./data --work ./reports --runtime python-skill -c "
import openpyxl
wb = openpyxl.Workbook()
wb.active.title = 'Summary'
wb.save('report.xlsx')
print('Report generated')
" --json
```

#### Pattern C: Safe Copy-on-Write Staging
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

#### Pattern D: Creating & Manipulating Output Files
Provide an explicit `--work` directory to write generated code, artifacts, or logs:
```bash
cocoon run --work ./agent_output --runtime node -c "
const fs = require('fs');
fs.writeFileSync('result.json', JSON.stringify({ status: 'completed', timestamp: Date.now() }));
" --json
```

#### Pattern E: Explicit Network Opt-In (Scraping / APIs)
Network is blocked by default (`allow_network: false`). If your task explicitly requires outbound HTTP requests, pass `--allow-network`:
```bash
cocoon run --allow-network --runtime node -c "
const https = require('https');
https.get('https://api.github.com/zen', { headers: { 'User-Agent': 'Cocoon' } }, res => {
  res.pipe(process.stdout);
});
" --json
```

#### Pattern F: Heavy Data Processing (Massive Excel / CSV / Parquet)
When extracting hundreds of thousands of rows from large spreadsheets (50MB–1GB+), use `-r python-skill`, extend `--timeout`, increase `--disk`, and write the results to a file in `--work`:
```bash
cocoon run \
  --ro ./data_warehouse \
  --work ./output_dir \
  --timeout 180 \
  --disk 2048 \
  -r python-skill \
  -c "
import pandas as pd
# Read from read-only host mount at native NVMe speed (>3 GB/s)
df = pd.read_excel('data_warehouse/large_ledger_500MB.xlsx', engine='calamine')
# Filter rows matching criteria
high_value = df[df['amount'] > 50000]
# Save output to workspace
high_value.to_parquet('output_dir/filtered_ledger.parquet')
print(f'Extracted {len(high_value)} rows out of {len(df)} total rows.')
" --json
```

#### Pattern G: Converting Hundreds of PDF Pages to Word / Text
Converting 300+ page documents requires extending the timeout and ensuring sufficient disk quota for converted assets:
```bash
cocoon run \
  --ro ./documents \
  --work ./extracted_text \
  --timeout 240 \
  --disk 1024 \
  -r python-skill \
  -c "
from pypdf import PdfReader
from docx import Document

reader = PdfReader('documents/quarterly_filing_400_pages.pdf')
doc = Document()
for page in reader.pages:
    txt = page.extract_text()
    if txt:
        doc.add_paragraph(txt)
doc.save('extracted_text/filing.docx')
print(f'Converted {len(reader.pages)} PDF pages to Word docx.')
" --json
```

#### Pattern H: Multimedia Processing (ffmpeg & Speech-to-Text)
Run CLI tools like `ffmpeg` or Python ML models (Whisper/Torch) directly in Cocoon:
```bash
# 1. Audio Extraction via ffmpeg
cocoon run --ro ./videos --work ./audio --timeout 300 --disk 4096 -- \
  ffmpeg -i videos/keynote_4k.mp4 -vn -acodec libmp3lame -q:a 2 audio/extracted_audio.mp3

# 2. Transcription with local Whisper in python-skill
cocoon run --ro ./audio --work ./transcripts --timeout 300 -r python-skill -c "
import whisper
model = whisper.load_model('base')
res = model.transcribe('audio/extracted_audio.mp3')
open('transcripts/transcription.txt', 'w', encoding='utf-8').write(res['text'])
print(f'Transcription complete: {len(res[\"text\"])} characters.')
" --json
```

#### Pattern I: Local SLM Inference & Fine-Tuning (GPU vs. CPU)
Run local Small Language Models (SLMs) such as Phi-3, Llama-3-8B, or Mistral-7B, or train LoRA adapters with mathematical host protection:
```bash
# 1. GPU-accelerated SLM inference with automatic VRAM fragmentation protection:
cocoon run --ro ./models --work ./output --timeout 180 --disk 4096 -r python-skill -c "
from transformers import AutoModelForCausalLM, AutoTokenizer
model = AutoModelForCausalLM.from_pretrained('models/Phi-3-mini', device_map='auto')
print('SLM loaded successfully onto GPU.')
" --json

# 2. Run strictly on CPU with complete GPU hardware detachment:
cocoon run --no-gpu --ro ./models --work ./output --timeout 180 -r python-skill -c "
import torch
print('CUDA Available:', torch.cuda.is_available()) # False
" --json
```

---

### ⚠️ Essential Tips for Heavy Workloads

1. **The "File-to-File Processing" Rule:**  
   Never print massive extracts to standard output (`stdout`). Cocoon enforces a **10 MB streaming buffer limit** to protect agent harnesses from memory exhaustion. If a script prints 100,000 rows to stdout, output is truncated (`stdout_truncated: true`). Always write output to a file inside `--work` (`.parquet`, `.csv`, `.jsonl`, `.docx`) and print a concise 1-line summary to stdout.

2. **Zero-Copy NVMe Speed:**  
   Unlike Docker or virtual machines which suffer from VirtIO-FS translation bottlenecks (300–800 MB/s), Cocoon mounts read-only files (`--ro`) via native OS kernel primitives (Windows Deny ACEs / Linux mount namespaces). Reading a 5 GB video or a 500 MB Excel file happens at **raw NVMe bus speed (3–7 GB/s)** with zero memory copy.

3. **Tuning Limits for Heavy Workloads:**
   - **Timeout:** Default is `30s`. Increase via `--timeout 180` (3 mins) or `--timeout 600` (10 mins).
   - **Disk Quota:** Default is `512MB`. Increase via `--disk 2048` (2 GB) or `--disk 10240` (10 GB).
   - **Memory (RAM):** Auto-detected from host physical RAM and hard-capped at **95% MAX**. Set `--mem <MB>` if you want an even lower limit.
   - **CPU:** Automatically clamped to **95% MAX of physical CPU capacity** to prevent host OS starvation. Override with `--cpu <PCT>`.
   - **GPU Isolation:** By default, GPU is permitted with automatic PyTorch VRAM allocator fragmentation guard (`PYTORCH_CUDA_ALLOC_CONF="max_split_size_mb:128"`). Pass `--no-gpu` to completely mask and detach all GPU hardware drivers.
   - **Runtime:** Always use `-r python-skill` (not `-r python`) for scripts importing `pandas`, `openpyxl`, `docx`, `pdfplumber`, `transformers`, or `torch`.

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
    { "step": 1, "runtime": "python", "exit_code": 0, "stdout": "", "stderr": "", "duration_ms": 190.2, "timed_out": false, "oom_killed": false, "stdout_truncated": false, "stderr_truncated": false },
    { "step": 2, "runtime": "bun", "exit_code": 0, "stdout": "", "stderr": "", "duration_ms": 18.5, "timed_out": false, "oom_killed": false, "stdout_truncated": false, "stderr_truncated": false },
    { "step": 3, "runtime": "pwsh", "exit_code": 0, "stdout": "{\"data\":[10,20,30],\"total\":60}\r\n", "stderr": "", "duration_ms": 76.7, "timed_out": false, "oom_killed": false, "stdout_truncated": false, "stderr_truncated": false }
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
Supported presets: `python`, `python-skill`, `node`, `bun`, `deno`, `go`, `java`, `rust` / `cargo`, `pwsh` / `powershell`, `bash` / `sh`, `ruby`, `php`, `lua`.

---

## 🔒 Built-in Sandbox Invariants (Enforced by Default)

1. **Default-Deny Network (`allow_network: false`):**
   * **Linux:** Bubblewrap severs all network namespaces (`CLONE_NEWNET`). Outbound TCP/UDP sockets fail with `Network is unreachable`.
   * **Windows:** Standard proxy variables (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`) sink to `127.0.0.1:0`. Cooperative HTTP clients (`requests`, `urllib`, `fetch`, `curl`, `pip`) fail immediately. *Note:* Raw `socket.connect()` calls that bypass proxy settings are not blocked on bare-metal Windows; use Docker/WSL2 or firewall rules if strict socket severance is required.

2. **Read-Only by Default (`--ro .`):**
   * The current working directory is automatically mounted as **strictly read-only** by default.
   * On Linux, mount namespaces enforce read-only status. On Windows, NTFS Deny ACEs block write/append/delete operations on designated folders (`PermissionError: Permission denied`). Ambient host paths outside `--ro` retain standard user read access.

3. **Zero Host Writes & Workspace Access Control:**
   * Code can **never** get write access to anything other than the designated `--work` folder (or the auto-cleaned ephemeral scratchpad).
   * Workspace DACLs are strictly bounded to the executing user (`%USERNAME%`), never `Everyone` (`*S-1-1-0`).
   * To edit or refactor existing code, use `--copy <path>` to stage a copy into the work folder and manipulate it there safely.

4. **Desktop & Clipboard Isolation:**
   * Win32 Job Object UI restrictions prevent child processes from accessing the user clipboard (`GetClipboardData`), injecting synthetic keystrokes, or intercepting window handles.

5. **30-Second Execution Timeout by Default:**
   * Every execution is automatically capped at **30 seconds** by default (override with `--timeout <secs>`). Infinite loops or hangs auto-terminate without freezing the agent.

6. **Process & Storage Caps:**
   * Active processes capped at 32 (`--max-procs 32`) to prevent fork bombs.
   * Disk write quota capped at 512 MB (`--disk 512`) to prevent disk exhaustion (enforced in-flight via `RLIMIT_FSIZE` on Linux and verified on teardown on Windows).

7. **Universal Breakout Verification:**
   * Post-run canonicalization scans detect and reject any symlink, NTFS junction (`0x400`), or hardlink attempting to point outside the workspace root.

8. **Hardware-Aware 95% Ceiling:**
   * Automatically inspects host physical RAM (`GlobalMemoryStatusEx` on Windows, `sysconf` on Linux) and caps memory usage to **95% of physical hardware**, guaranteeing that the host OS never enters paging thrash or crashes.

9. **Scheduler Lockout & Priority Elevation Hardening:**
   * Process priority is locked to `NORMAL_PRIORITY_CLASS` on Windows and real-time priorities are blocked (`RLIMIT_RTPRIO=0`) on Linux, preventing guest code from starving the host OS scheduler or system drivers. CPU is hard-capped at 95% MAX.

10. **GPU Detachment & VRAM Safety (`--no-gpu`):**
    * When `--no-gpu` is passed, all GPU driver interfaces (NVIDIA CUDA, AMD ROCm/HIP, Intel, OpenCL) are detached. When enabled, automatic VRAM allocator fragmentation guards are injected.

11. **Core Dump Suppression:**
    * Injects `RLIMIT_CORE=0` on Linux and suppresses WER popups on Windows to prevent multi-gigabyte core dumps and credential/prompt leaks.

12. **Chassis Policy vs. Model Agency:**
    * The host chassis, not the LLM, must choose the execution flags (`--allow-network`, `-r python-skill`, `--work`). The agent tool schema should only accept `code: string`.

13. **Verify Telemetry in `--json`:**
    * Always verify `result.exit_code === 0` and `result.timed_out === false` before consuming `result.stdout`. Check `result.termination_reason` if execution fails.
