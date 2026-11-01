"""Cocoon Sandbox Python Client SDK.

A featherweight, cross-platform sandbox with virtual and read-only filesystems
designed for AI agents and secure code execution.
"""

from .client import (
    ExecutionResult,
    PipelineResult,
    PipelineStepResult,
    doctor,
    get_cocoon_binary,
    pipeline,
    run,
)

__version__ = "0.2.0"
__all__ = [
    "run",
    "pipeline",
    "doctor",
    "get_cocoon_binary",
    "ExecutionResult",
    "PipelineResult",
    "PipelineStepResult",
]

