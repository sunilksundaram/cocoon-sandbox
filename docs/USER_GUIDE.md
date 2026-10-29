# Cocoon Integration & User Guide 📖

> A complete guide for using Cocoon as a standalone sandbox, with major agent frameworks (Microsoft Agent Framework, LangChain, LangGraph, LlamaIndex), and agent harnesses (Claude Desktop, GitHub Copilot, Hermes, Cursor, Antigravity, and custom agent loops).

👉 *Need setup or prerequisite instructions? See the [Installation Guide](INSTALLATION.md).*

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
# 1. Safe execution (current directory is read-only by default, 30s timeout)
cocoon run -r python -c "print(2 + 2)"

# 2. Run inside an explicit writable workspace
cocoon run --work ./my_workspace -r node -c "require('fs').writeFileSync('out.txt', 'done')"

# 3. Copy-on-Write: Stage repository files into workspace and manipulate them safely
cocoon run --copy ./config.json --work ./workspace -r python -c "
import json
d = json.load(open('config.json'))
d['active'] = True
json.dump(d, open('config.json', 'w'))
"

# 4. Polyglot Multi-Language Pipeline (Python -> Node -> Bun in the SAME virtual workspace)
cocoon pipeline \
  -s "python:open('data.txt', 'w').write('hello from python')" \
  -s "node:console.log('Read by Node:', require('fs').readFileSync('data.txt', 'utf8'))" \
  -s "bun:console.log('Read by Bun:', await Bun.file('data.txt').text())" \
  --json

# 5. Inspect available runtimes on host
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
        "enum": ["python", "node", "bun", "deno", "go", "java", "rust", "pwsh", "bash", "ruby", "php", "lua"],
        "description": "The programming language runtime to execute."
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
def execute(runtime: str, code: str, timeout: int = 30) -> str:
    """Run sandboxed code in Python, Node, Bun, Go, PowerShell, etc. Host files are read-only."""
    p = subprocess.run(
        ["cocoon", "run", "-r", runtime, "-c", code, "--timeout", str(timeout), "--json"],
        capture_output=True,
        text=True
    )
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
