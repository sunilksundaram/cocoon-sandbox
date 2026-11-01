from dataclasses import dataclass
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from typing import Any, Callable, Dict, List, Optional, Union

@dataclass
class ExecutionResult:
    """Structured telemetry output from a sandboxed Cocoon execution."""
    exit_code: int
    stdout: str
    stderr: str
    duration_ms: float
    timed_out: bool
    oom_killed: bool
    peak_memory_mb: Optional[int] = None
    stdout_truncated: bool = False
    stderr_truncated: bool = False
    termination_reason: Optional[str] = None

    @property
    def success(self) -> bool:
        """Returns True if the process completed with exit code 0 and was not terminated."""
        return self.exit_code == 0 and not self.timed_out and not self.oom_killed

@dataclass
class PipelineStepResult:
    """Telemetry report for an individual step in a polyglot pipeline."""
    step: int
    runtime: str
    exit_code: int
    stdout: str
    stderr: str
    duration_ms: float
    timed_out: bool
    oom_killed: bool
    stdout_truncated: bool = False
    stderr_truncated: bool = False
    termination_reason: Optional[str] = None

@dataclass
class PipelineResult:
    """Comprehensive result of a multi-step polyglot pipeline."""
    success: bool
    total_duration_ms: float
    steps: List[PipelineStepResult]

def get_cocoon_binary() -> str:
    """Locates the compiled cocoon binary from environment, wheel distribution, or PATH."""
    env_bin = os.environ.get("COCOON_BIN")
    if env_bin and os.path.isfile(env_bin):
        return env_bin

    # Check directory of this package or adjacent python script directory
    pkg_dir = Path(__file__).resolve().parent
    binary_name = "cocoon.exe" if os.name == "nt" else "cocoon"

    candidates = [
        pkg_dir / binary_name,
        pkg_dir.parent / binary_name,
        Path(sys.executable).resolve().parent / binary_name,
        Path(sys.prefix) / "bin" / binary_name,
        Path(sys.prefix) / "Scripts" / binary_name,
    ]

    for cand in candidates:
        if cand.is_file():
            return str(cand)

    path_bin = shutil.which("cocoon")
    if path_bin:
        return path_bin

    raise FileNotFoundError(
        "The 'cocoon' executable was not found. Please install cocoon-sandbox or set the COCOON_BIN environment variable."
    )

def run(
    code: Optional[str] = None,
    file: Optional[Union[str, Path]] = None,
    runtime: str = "python",
    ro: Optional[List[Union[str, Path]]] = None,
    rw: Optional[List[Union[str, Path]]] = None,
    work: Optional[Union[str, Path]] = None,
    temp: Optional[Union[str, Path]] = None,
    copy: Optional[List[Union[str, Path]]] = None,
    timeout: int = 30,
    mem: Optional[int] = None,
    disk: Optional[int] = None,
    cpu: Optional[int] = None,
    max_procs: Optional[int] = None,
    allow_network: bool = False,
    no_gpu: bool = False,
    raw_cmd: Optional[List[str]] = None,
    stream: bool = False,
    on_chunk: Optional[Callable[[Dict[str, Any]], None]] = None,
) -> ExecutionResult:
    """Executes code or commands inside a secure, ultra-low-latency Cocoon sandbox.

    Args:
        code: Inline code string to execute.
        file: Path to script file to execute.
        runtime: Language runtime preset ('python', 'python-skill', 'node', 'bun', 'deno', 'go', 'rust', 'pwsh', etc.).
        ro: List of host paths to mount as strictly read-only.
        rw: List of host paths to mount as read-write.
        work: Explicit working directory for workspace files.
        temp: Custom temporary directory.
        copy: List of host files/directories to stage-copy into workspace prior to execution.
        timeout: Execution timeout in seconds (default: 30).
        mem: Maximum RAM limit in Megabytes.
        disk: Maximum disk quota in Megabytes (default: 512).
        cpu: Maximum CPU percentage cap (capped at 95% MAX).
        max_procs: Maximum active processes limit (default: 32).
        allow_network: Whether to permit outbound networking (default: False).
        no_gpu: Whether to detach and isolate GPU hardware.
        raw_cmd: Trailing arguments for arbitrary compilers or commands without preset.
        stream: Whether to stream NDJSON execution events.
        on_chunk: Callback invoked for each streamed NDJSON event when stream=True.

    Returns:
        ExecutionResult containing exit code, stdout, stderr, and telemetry.
    """
    cocoon_bin = get_cocoon_binary()
    cmd = [cocoon_bin, "run"]

    if ro:
        for r in ro:
            cmd.extend(["--ro", str(r)])
    if rw:
        for w in rw:
            cmd.extend(["--rw", str(w)])
    if work:
        cmd.extend(["--work", str(work)])
    if temp:
        cmd.extend(["--temp", str(temp)])
    if copy:
        for c in copy:
            cmd.extend(["--copy", str(c)])

    cmd.extend(["--timeout", str(timeout)])
    if mem is not None:
        cmd.extend(["--mem", str(mem)])
    if disk is not None:
        cmd.extend(["--disk", str(disk)])
    if cpu is not None:
        cmd.extend(["--cpu", str(cpu)])
    if max_procs is not None:
        cmd.extend(["--max-procs", str(max_procs)])
    if allow_network:
        cmd.append("--allow-network")
    if no_gpu:
        cmd.append("--no-gpu")

    if raw_cmd:
        cmd.append("--")
        cmd.extend(raw_cmd)
    else:
        cmd.extend(["--runtime", runtime])
        if code is not None:
            cmd.extend(["-c", code])
        elif file is not None:
            cmd.extend(["-f", str(file)])

    if stream:
        cmd.append("--stream")
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        accumulated_stdout: List[str] = []
        accumulated_stderr: List[str] = []
        last_complete: Dict[str, Any] = {}
        if proc.stdout:
            for line in proc.stdout:
                line_str = line.strip()
                if not line_str:
                    continue
                try:
                    event = json.loads(line_str)
                    if on_chunk:
                        on_chunk(event)
                    evt = event.get("event")
                    if evt == "stdout":
                        accumulated_stdout.append(event.get("data", ""))
                    elif evt == "stderr":
                        accumulated_stderr.append(event.get("data", ""))
                    elif evt == "complete":
                        last_complete = event
                except json.JSONDecodeError:
                    pass
        proc.wait()
        return ExecutionResult(
            exit_code=last_complete.get("exit_code", proc.returncode if proc.returncode is not None else 1),
            stdout="".join(accumulated_stdout),
            stderr="".join(accumulated_stderr),
            duration_ms=last_complete.get("duration_ms", 0.0),
            timed_out=last_complete.get("timed_out", False),
            oom_killed=last_complete.get("oom_killed", False),
            peak_memory_mb=last_complete.get("peak_memory_mb"),
            stdout_truncated=last_complete.get("stdout_truncated", False),
            stderr_truncated=last_complete.get("stderr_truncated", False),
            termination_reason=last_complete.get("termination_reason"),
        )
    else:
        cmd.append("--json")
        res = subprocess.run(cmd, capture_output=True, text=True)
        try:
            data = json.loads(res.stdout)
            return ExecutionResult(
                exit_code=data.get("exit_code", res.returncode),
                stdout=data.get("stdout", ""),
                stderr=data.get("stderr", res.stderr),
                duration_ms=data.get("duration_ms", 0.0),
                timed_out=data.get("timed_out", False),
                oom_killed=data.get("oom_killed", False),
                peak_memory_mb=data.get("peak_memory_mb"),
                stdout_truncated=data.get("stdout_truncated", False),
                stderr_truncated=data.get("stderr_truncated", False),
                termination_reason=data.get("termination_reason"),
            )
        except json.JSONDecodeError:
            return ExecutionResult(
                exit_code=res.returncode,
                stdout=res.stdout,
                stderr=res.stderr,
                duration_ms=0.0,
                timed_out=False,
                oom_killed=False,
            )

def pipeline(
    steps: List[str],
    ro: Optional[List[Union[str, Path]]] = None,
    work: Optional[Union[str, Path]] = None,
    temp: Optional[Union[str, Path]] = None,
    copy: Optional[List[Union[str, Path]]] = None,
    timeout: Optional[int] = None,
    allow_network: bool = False,
    mem: Optional[int] = None,
) -> PipelineResult:
    """Executes a multi-language polyglot pipeline in a shared virtual workspace."""
    cocoon_bin = get_cocoon_binary()
    cmd = [cocoon_bin, "pipeline", "--json"]

    if ro:
        for r in ro:
            cmd.extend(["--ro", str(r)])
    if work:
        cmd.extend(["--work", str(work)])
    if temp:
        cmd.extend(["--temp", str(temp)])
    if copy:
        for c in copy:
            cmd.extend(["--copy", str(c)])
    if timeout:
        cmd.extend(["--timeout", str(timeout)])
    if mem:
        cmd.extend(["--mem", str(mem)])
    if allow_network:
        cmd.append("--allow-network")

    for s in steps:
        cmd.extend(["--step", s])

    res = subprocess.run(cmd, capture_output=True, text=True)
    try:
        data = json.loads(res.stdout)
    except json.JSONDecodeError:
        data = {"success": False, "total_duration_ms": 0.0, "steps": []}

    step_results = []
    for s in data.get("steps", []):
        step_results.append(
            PipelineStepResult(
                step=s.get("step", 0),
                runtime=s.get("runtime", ""),
                exit_code=s.get("exit_code", 0),
                stdout=s.get("stdout", ""),
                stderr=s.get("stderr", ""),
                duration_ms=s.get("duration_ms", 0.0),
                timed_out=s.get("timed_out", False),
                oom_killed=s.get("oom_killed", False),
                stdout_truncated=s.get("stdout_truncated", False),
                stderr_truncated=s.get("stderr_truncated", False),
                termination_reason=s.get("termination_reason"),
            )
        )

    return PipelineResult(
        success=data.get("success", False),
        total_duration_ms=data.get("total_duration_ms", 0.0),
        steps=step_results,
    )

def doctor(json_output: bool = True) -> Dict[str, Any]:
    """Runs the environment diagnostic self-test."""
    cocoon_bin = get_cocoon_binary()
    cmd = [cocoon_bin, "doctor"]
    if json_output:
        cmd.append("--json")
    res = subprocess.run(cmd, capture_output=True, text=True)
    if json_output:
        try:
            return json.loads(res.stdout)
        except json.JSONDecodeError:
            return {"output": res.stdout, "error": res.stderr}
    return {"output": res.stdout}
