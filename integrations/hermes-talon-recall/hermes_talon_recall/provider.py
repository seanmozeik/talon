"""TalonRecallProvider: Hermes MemoryProvider backed by `talon recall`.

Talon is recall-only and stateless per call. This plugin:
  - Implements prefetch() synchronously — always uses the current query.
  - Returns "" on timeout (20s), non-zero exit, empty output, or skipped response.
  - Buffers recent user turns via sync_turn() so --prior-message widens the query.
  - Never writes to the vault.
"""

from __future__ import annotations

import json
import logging
import os
import shutil
import subprocess
from collections import deque
from pathlib import Path
from typing import Any

from agent.memory_provider import MemoryProvider

logger = logging.getLogger(__name__)

_INSTALL_HINT = (
    "Install Talon: https://github.com/seanmozeik/talon  "
    "or set TALON_BIN to the absolute binary path."
)
_NO_RECALL = ""
_SKIPPED_PREFIX = '<vault_recall skipped="true"'
_TIMEOUT = 20  # seconds


class TalonRecallProvider(MemoryProvider):
    """Hermes MemoryProvider — vault-native context via talon recall --format prompt-xml."""

    @property
    def name(self) -> str:
        return "talon-recall"

    def __init__(self) -> None:
        self._binary: str | None = None
        self._vault_path: str | None = None
        self._budget_tokens: int = 500
        self._min_confidence: float = 0.4
        self._fast: bool = False
        self._prior_message_count: int = 2
        # Stores user message strings only — assistant content not useful for BM25 expansion.
        self._turn_history: deque[str] = deque(maxlen=8)

    # ── MemoryProvider ABC ────────────────────────────────────────────────────

    def is_available(self) -> bool:
        return self._resolve_binary() is not None

    def initialize(self, session_id: str, **kwargs: Any) -> None:
        binary = self._resolve_binary()
        if binary is None:
            raise RuntimeError(f"talon binary not found. {_INSTALL_HINT}")
        self._binary = binary
        self._load_config(kwargs.get("hermes_home", ""))
        if vault_env := os.environ.get("TALON_VAULT"):
            self._vault_path = vault_env

    def system_prompt_block(self) -> str:
        return (
            "# Talon Vault\n"
            "Relevant vault notes are auto-injected as <vault_recall> before each turn. "
            "Use `talon read` or `talon search` via the shell to look up notes directly."
        )

    def prefetch(self, query: str, *, session_id: str = "") -> str:
        """Run talon recall synchronously. Returns vault_recall XML or empty string.

        Empty string is returned (cache-safe, no injection) on:
          - 20s timeout
          - non-zero exit code
          - empty stdout
          - skipped=true confidence-gate response
        """
        if self._binary is None:
            return _NO_RECALL
        try:
            result = subprocess.run(
                self._build_command(query),
                capture_output=True,
                text=True,
                timeout=_TIMEOUT,
                env=self._build_env(),
            )
        except subprocess.TimeoutExpired:
            logger.debug("talon-recall: prefetch timed out after %ds", _TIMEOUT)
            return _NO_RECALL
        except Exception as exc:
            logger.warning("talon-recall: subprocess error: %s", exc)
            return _NO_RECALL

        if result.returncode != 0:
            logger.warning(
                "talon-recall: exited %d: %s", result.returncode, result.stderr[:200]
            )
            return _NO_RECALL

        stdout = result.stdout.strip()
        if not stdout or stdout.startswith(_SKIPPED_PREFIX):
            return _NO_RECALL

        return stdout

    def sync_turn(
        self, user_content: str, assistant_content: str, *, session_id: str = ""
    ) -> None:
        """Buffer user message for --prior-message expansion on the next prefetch."""
        if user_content.strip():
            self._turn_history.append(user_content)

    def get_tool_schemas(self) -> list[dict[str, Any]]:
        return []

    def shutdown(self) -> None:
        pass

    # ── config ────────────────────────────────────────────────────────────────

    def get_config_schema(self) -> list[dict[str, Any]]:
        return [
            {
                "key": "vault_path",
                "description": "Absolute path to your Obsidian vault directory",
                "required": False,
                "env_var": "TALON_VAULT",
            },
            {
                "key": "budget_tokens",
                "description": "Token budget for the recall context block (default 500)",
                "default": 500,
            },
            {
                "key": "min_confidence",
                "description": "Minimum evidence score 0.0–1.0 (default 0.4)",
                "default": 0.4,
            },
            {
                "key": "fast",
                "description": "Skip LLM expansion and reranking (default false)",
                "default": False,
            },
            {
                "key": "prior_message_count",
                "description": "Recent user turns fed via --prior-message (default 2)",
                "default": 2,
            },
        ]

    def save_config(self, values: dict[str, Any], hermes_home: str) -> None:
        Path(hermes_home).joinpath("talon-recall.json").write_text(
            json.dumps(values, indent=2)
        )

    # ── private helpers ───────────────────────────────────────────────────────

    def _resolve_binary(self) -> str | None:
        if bin_env := os.environ.get("TALON_BIN"):
            if os.path.isfile(bin_env) and os.access(bin_env, os.X_OK):
                return bin_env
            return None
        return shutil.which("talon")

    def _load_config(self, hermes_home: str) -> None:
        if not hermes_home:
            return
        config_path = Path(hermes_home) / "talon-recall.json"
        if not config_path.exists():
            return
        try:
            cfg: dict[str, Any] = json.loads(config_path.read_text())
        except Exception as exc:
            logger.warning("talon-recall: failed to read config: %s", exc)
            return
        self._vault_path = cfg.get("vault_path") or self._vault_path
        self._budget_tokens = int(cfg.get("budget_tokens", self._budget_tokens))
        self._min_confidence = float(cfg.get("min_confidence", self._min_confidence))
        self._fast = bool(cfg.get("fast", self._fast))
        self._prior_message_count = int(
            cfg.get("prior_message_count", self._prior_message_count)
        )

    def _build_command(self, query: str) -> list[str]:
        assert self._binary is not None
        cmd = [
            self._binary, "recall", query,
            "--format", "prompt-xml",
            "--budget-tokens", str(self._budget_tokens),
            "--min-confidence", str(self._min_confidence),
        ]
        for msg in list(self._turn_history)[-self._prior_message_count:]:
            cmd += ["--prior-message", msg]
        if self._fast:
            cmd.append("--fast")
        return cmd

    def _build_env(self) -> dict[str, str]:
        env = os.environ.copy()
        if self._vault_path:
            env["TALON_VAULT"] = self._vault_path
        return env


def register(ctx) -> None:
    ctx.register_memory_provider(TalonRecallProvider())
