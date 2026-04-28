"""Hermes Memory Provider plugin: talon-recall.

Wraps `talon recall --format prompt-xml` to surface vault-native context from
an Obsidian knowledge base into Hermes Agent's turn context.
"""

from .provider import TalonRecallProvider, register


__all__ = ["TalonRecallProvider", "register"]
