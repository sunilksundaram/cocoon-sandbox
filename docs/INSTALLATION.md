# Cocoon Installation Guide 🛠️

> Complete installation guidelines, platform prerequisites, verification steps, and troubleshooting for **Cocoon** across Windows, Linux, and macOS.

---

## 📋 Table of Contents
1. [System Requirements](#1-system-requirements)
2. [Quick Installation](#2-quick-installation)
3. [Installing from Source (Cargo)](#3-installing-from-source-cargo)
4. [Platform-Specific Setup](#4-platform-specific-setup)
   * [Windows Setup](#windows-10--11)
   * [Linux Setup](#linux-ubuntudebian-fedora-arch-alpine)
   * [macOS Setup](#macos-apple-silicon--intel)
5. [Verifying Installation](#5-verifying-installation)
6. [Configuring Language Runtimes](#6-configuring-language-runtimes)
7. [Agent Skill & MCP Server Integration](#7-agent-skill--mcp-server-integration)
8. [Troubleshooting & FAQ](#8-troubleshooting--faq)

---

## 1. System Requirements

Cocoon is a **native systems binary** written in Rust. It does **not** require Docker, virtual machines, hypervisors, or root/administrator privileges.

| Operating System | Supported Architectures | Minimum Version | Native Isolation Technology |
| :--- | :--- | :--- | :--- |
| **Windows** | x86_64, ARM64 | Windows 10 build 1809+, Windows 11 | Windows Job Objects & NTFS Inherited ACLs |
| **Linux** | x86_64, aarch64, armv7 | Linux Kernel 4.18+ | Unprivileged User Namespaces & Bubblewrap |
| **macOS** | Apple Silicon (M1–M4), Intel x86_64 | macOS 12 Monterey+ | Apple Seatbelt Framework (`libsandbox`) |

---

## 2. Quick Installation

### Option A: Via Python / PyPI (Instant Pre-Compiled Binary)
```bash
pip install cocoon-sandbox
```
*(Installs the standalone `cocoon` native binary into your active Python environment's path).*

### Option B: Via Cargo / Crates.io
```bash
cargo install cocoon-sandbox
```
*(Builds and installs `cocoon` directly into `~/.cargo/bin` or `%USERPROFILE%\.cargo\bin`).*

### Option C: Local Project Installation
If you have cloned the repository locally:

```bash
cd cocoon
cargo install --path crates/cocoon-cli
```
*(Installs `cocoon` directly to `~/.cargo/bin/cocoon` or `%USERPROFILE%\.cargo\bin\cocoon.exe`).*

---

## 3. Installing from Source (Cargo)

### Step 1: Install Rust Toolchain
If Rust is not already installed on your system, install it via the official [rustup](https://rustup.rs/) installer:

* **Windows (PowerShell):**
  ```powershell
  winget install Rustlang.Rustup
  # or download from https://rustup.rs/
  ```
* **Linux & macOS:**
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

### Step 2: Clone and Build
```bash
git clone https://github.com/sunilksundaram/cocoon.git
cd cocoon

# Build optimized release binary
cargo build --release

# The compiled binary is now ready at:
# Linux/macOS: target/release/cocoon
# Windows:     target/release/cocoon.exe
```

### Step 3: Add to System PATH
To install the binary globally so you can type `cocoon` from any directory:
```bash
cargo install --path crates/cocoon-cli
```

Ensure your `~/.cargo/bin` (or `%USERPROFILE%\.cargo\bin` on Windows) is in your system's `PATH`.

---

## 4. Platform-Specific Setup

### Windows (10 / 11)
* **Zero Driver Setup:** Cocoon uses Windows Job Objects and native NTFS Access Control Lists (ACLs). No external drivers or WSL required.
* **No Administrator Rights Needed:** Any standard user account can run Cocoon and protect their workspace files.
* **Path Verification (PowerShell):**
  ```powershell
  # Check if cargo bin is in your path
  $env:PATH -split ';' | Select-String "cargo"
  ```
  If missing, add it to your user environment:
  ```powershell
  [Environment]::SetEnvironmentVariable("PATH", $env:PATH + ";$HOME\.cargo\bin", "User")
  ```

### Linux (Ubuntu/Debian, Fedora, Arch, Alpine)
Cocoon runs unprivileged in user namespaces.
* **User Namespaces:** Modern Linux distributions have unprivileged user namespaces enabled by default. To verify:
  ```bash
  sysctl kernel.unprivileged_userns_clone
  # Should output: kernel.unprivileged_userns_clone = 1
  ```
* **Optional Bubblewrap Utility (`bwrap`):**
  Cocoon can optionally utilize Bubblewrap for hardware device isolation:
  * **Ubuntu / Debian:** `sudo apt install bubblewrap`
  * **Fedora:** `sudo dnf install bubblewrap`
  * **Arch Linux:** `sudo pacman -S bubblewrap`
  * **Alpine Linux:** `sudo apk add bubblewrap`

### macOS (Apple Silicon & Intel)
* **Zero Configuration:** Uses Apple's kernel-level Seatbelt Sandbox (`libsandbox`). No third-party packages or system extensions needed.
* Works seamlessly with Homebrew toolchains located in `/opt/homebrew` or `/usr/local`.

---

## 5. Verifying Installation

Verify that the CLI is accessible and inspect your local execution drivers:

```bash
cocoon --version
```
**Expected Output:**
```text
cocoon 0.1.0
```

Now run `cocoon info` to inspect the active driver and detected system runtimes:

```bash
cocoon info
```

**Example Output:**
```text
Cocoon Sandbox v0.1.0
----------------------------------------
Active Driver : windows-job-object
Driver Status : Available (Active)
Target OS     : windows
Target Arch   : x86_64

Detected System Runtimes:
  - Python       : ✓ Ready
  - Node.js      : ✓ Ready
  - Bun          : ✓ Ready
  - Deno         : ✓ Ready
  - Go           : ✓ Ready
  - Java         : ✓ Ready
  - Rust/Cargo   : ✓ Ready
  - PowerShell   : ✓ Ready
  - Bash/Shell   : ✓ Ready
  - Ruby         : ✓ Ready
  - PHP          : ✓ Ready
  - Lua          : ✓ Ready
```

### Run a Smoke Test:
```bash
# Test inline execution with default 30s timeout and default read-only host protection
cocoon run -r python -c "import sys; print('Cocoon operational on', sys.platform)"
```

---

## 6. Configuring Language Runtimes

Cocoon executes runtimes installed on your host machine. Install only the runtimes your agents need:

### Python
* Windows: `winget install Python.Python.3.12` or `winget install Python.Python.3.14`
* macOS: `brew install python`
* Linux: `sudo apt install python3 python3-pip python3-venv`

### Node.js & Bun
* Node: [nodejs.org](https://nodejs.org/) or `brew install node` / `sudo apt install nodejs npm`
* Bun: `powershell -c "irm bun.sh/install.ps1 | iex"` or `curl -fsSL https://bun.sh/install | bash`

### Go
* Windows: `winget install GoLang.Go`
* macOS: `brew install go`
* Linux: `sudo apt install golang`

### PowerShell (pwsh)
* Windows: Built-in Windows PowerShell or PowerShell 7 (`winget install Microsoft.PowerShell`)
* macOS: `brew install --cask powershell`
* Linux: `sudo apt install powershell`

---

## 7. Agent Skill & MCP Server Integration

### Step 1: Install the Agent Skill
To make Cocoon automatically discoverable by AI assistants and coding agents:

* **Workspace Level:** Copy [`SKILL.md`](../.agents/skills/cocoon-sandbox/SKILL.md) into your repository's `.agents/skills/cocoon-sandbox/SKILL.md`.
* **Global Level:** Copy `SKILL.md` into `~/.gemini/config/skills/cocoon-sandbox/SKILL.md` (or `%USERPROFILE%\.gemini\config\skills\cocoon-sandbox\SKILL.md` on Windows).

### Step 2: Configure Desktop Harnesses (Claude Desktop, Cursor, Antigravity)
Add Cocoon as a local Model Context Protocol (MCP) tool in your `claude_desktop_config.json` or `mcp_config.json`:

```json
{
  "mcpServers": {
    "cocoon": {
      "command": "cocoon",
      "args": ["run", "--json"]
    }
  }
}
```

---

## 8. Troubleshooting & FAQ

### Q1: `cocoon: command not found` or `The term 'cocoon' is not recognized`
* **Cause:** Cargo's global binary directory is not in your system `PATH`.
* **Solution:**
  * **Windows:** Add `%USERPROFILE%\.cargo\bin` to your Environment Variables, then restart PowerShell.
  * **Linux / macOS:** Add `export PATH="$HOME/.cargo/bin:$PATH"` to your `~/.bashrc` or `~/.zshrc`.

### Q2: `PermissionError: [Errno 13] Permission denied` when my script runs
* **Explanation:** This is **by design**. Host directories are strictly read-only (`--ro`) by default.
* **Fix:** If your script needs to write files or create outputs, specify a dedicated work directory using `--work`:
  ```bash
  cocoon run --work ./my_output -r python -c "open('test.txt', 'w').write('ok')"
  ```
  Or stage existing files into the workspace with `--copy`:
  ```bash
  cocoon run --copy ./config.json --work ./my_output -r python script.py
  ```

### Q3: `Process terminated: execution timed out`
* **Explanation:** Cocoon enforces a default **30-second hard timeout** to kill infinite loops.
* **Fix:** For long-running compilation or heavy scripts, extend the timeout with `--timeout`:
  ```bash
  cocoon run --timeout 120 -r go -f heavy_build.go
  ```

### Q4: Windows file locking error during `cargo build` (`os error 32`)
* **Cause:** Another process or Windows Defender briefly inspected `target/debug/cocoon.exe` right after execution.
* **Fix:** Wait 2 seconds and re-run, or add your project folder to Windows Defender's process exclusion list for Cargo.
