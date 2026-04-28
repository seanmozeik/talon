"""Hermes memory-provider discovery shim for talon-recall."""

from agent.memory_provider import MemoryProvider as _MemoryProvider
from hermes_talon_recall import TalonRecallProvider, register

assert issubclass(TalonRecallProvider, _MemoryProvider)

__all__ = ["TalonRecallProvider", "register"]
