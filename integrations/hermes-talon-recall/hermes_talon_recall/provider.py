"""TalonRecallProvider: Hermes MemoryProvider backed by `talon recall`.

Talon is recall-only and stateless per call. This plugin:
  - Implements prefetch() to run `talon recall --format prompt-xml` before each turn.
  - Buffers recent turns via sync_turn() so --prior-message widens the query.
  - Returns "" on all failure paths so the agent never sees malformed context.
  - Does NOT implement ingest/write-back; the agent host owns vault mutations.
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

logger = logging.getLogger(__name__)

_INSTALL_HINT = (
    "Install Talon: https://github.com/seanmozeik/talon  "
    "or set TALON_BIN to the absolute binary path."
)

# Sentinel returned by prefetch() when no context should be injected.
_NO_RECALL = ""

# XML tag prefix that signals a skipped/confidence-gated response.
_SKIPPED_PREFIX = '<vault_recall skipped="true"'


class TalonRecallProvider:
    """Hermes MemoryProvider that shells out to `talon recall --format prompt-xml`.

    Configuration (set via `hermes memory setup talon-recall` or ~/.hermes/talon-recall.json):
      vault_path            Obsidian vault directory (falls back to TALON_VAULT env var).
      budget_tokens         Token budget passed to talon (default 2000).
      min_confidence        Minimum evidence score; below this returns empty context (default 0.3).
      recency_half_life_days  Half-life for recency decay weighting (default 7).
      fast                  Skip LLM expansion and reranking (default False).
      prior_message_count   Number of recent user turns to feed via --prior-message (default 2).
    """

    @property
    def name(self) -> str:
        return "talon-recall"

    def __init__(self) -> None:
        self._binary: str | None = None
        self._vault_path: str | None = None
        self._budget_tokens: int = 2000
        self._min_confidence: float = 0.3
        self._recency_half_life_days: int = 7
        self._fast: bool = False
        self._prior_message_count: int = 2
        # Rolling buffer of (user_content, assistant_content) tuples.
        # Capped at 8; trimmed to prior_message_count at build time.
        self._turn_history: deque[tuple[str, str]] = deque(maxlen=8)

    # ── MemoryProvider ABC ────────────────────────────────────────────

    def is_available(self) -> bool:
        """Return True if the talon binary is discoverable (no network calls)."""
        return self._resolve_binary() is not None

    def initialize(self, session_id: str, **kwargs: Any) -> None:
        """Validate binary presence and load persisted config.

        Raises RuntimeError if the talon binary cannot be found so the error
        surfaces at plugin-load time, not silently at first prefetch.
        """
        binary = self._resolve_binary()
        if binary is None:
            raise RuntimeError(f"talon binary not found. {_INSTALL_HINT}")
        self._binary = binary

        hermes_home: str = kwargs.get("hermes_home", "")
        self._load_config(hermes_home)

        # TALON_VAULT env var takes precedence over saved config.
        if vault_env := os.environ.get("TALON_VAULT"):
            self._vault_path = vault_env

    def system_prompt_block(self) -> str:
        return (
            "You have access to vault-native context from an Obsidian knowledge base (Talon). "
            "Context blocks labelled <vault_recall> are retrieved automatically before each turn. "
            "Use this context to ground answers with your actual notes and knowledge base."
        )

    def prefetch(self, query: str, *, session_id: str = "") -> str:
        """Run `talon recall --format prompt-xml` and return the XML block.

        Returns "" (no-recall) on any failure: binary missing, non-zero exit,
        empty stdout, or a skipped=true confidence-gate response.
        Failures are logged at WARNING level; the agent never sees stack traces.
        """
        if self._binary is None:
            logger.warning("talon-recall: binary not initialized; skipping prefetch")
            return _NO_RECALL

        cmd = self._build_command(query)
        try:
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=30,
                env=self._build_env(),
            )
        except Exception as exc:
            logger.warning("talon-recall: subprocess error: %s", exc)
            return _NO_RECALL

        if result.returncode != 0:
            logger.warning(
                "talon-recall: exited %d: %s",
                result.returncode,
                result.stderr[:200],
            )
            return _NO_RECALL

        stdout = result.stdout.strip()
        if not stdout:
            return _NO_RECALL

        # Confidence gate: talon returns a self-closing tag when evidence_score
        # is below --min-confidence.  Return empty so agent context stays clean.
        if stdout.startswith(_SKIPPED_PREFIX):
            logger.debug("talon-recall: skipped=true (evidence below threshold)")
            return _NO_RECALL

        return stdout

    def sync_turn(
        self, user_content: str, assistant_content: str, *, session_id: str = ""
    ) -> None:
        """Buffer the completed turn for --prior-message on the next prefetch.

        Talon is recall-only; this method never writes to the vault.
        """
        self._turn_history.append((user_content, assistant_content))

    def get_tool_schemas(self) -> list[dict[str, Any]]:
        """Talon is recall-only; no agent tools are exposed."""
        return []

    def shutdown(self) -> None:
        """No-op: the talon binary is stateless with no persistent connections."""

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
                "description": "Token budget for the recall context block (default 2000)",
                "default": 2000,
            },
            {
                "key": "min_confidence",
                "description": (
                    "Minimum evidence score threshold 0.0–1.0; "
                    "below this returns empty context (default 0.3)"
                ),
                "default": 0.3,
            },
            {
                "key": "recency_half_life_days",
                "description": "Half-life in days for recency decay weighting (default 7)",
                "default": 7,
            },
            {
                "key": "fast",
                "description": (
                    "Skip LLM expansion and reranking for faster recall "
                    "at the cost of quality (default false)"
                ),
                "default": False,
            },
            {
                "key": "prior_message_count",
                "description": (
                    "Number of recent user turns to feed via --prior-message "
                    "to widen the implicit query (default 2)"
                ),
                "default": 2,
            },
        ]

    def save_config(self, values: dict[str, Any], hermes_home: str) -> None:
        """Write non-secret config to $HERMES_HOME/talon-recall.json."""
        config_path = Path(hermes_home) / "talon-recall.json"
        config_path.write_text(json.dumps(values, indent=2))

    # ── private helpers ───────────────────────────────────────────────

    def _resolve_binary(self) -> str | None:
        """Return the talon binary path, or None if not discoverable."""
        if bin_env := os.environ.get("TALON_BIN"):
            if os.path.isfile(bin_env) and os.access(bin_env, os.X_OK):
                return bin_env
            return None
        return shutil.which("talon")

    def _load_config(self, hermes_home: str) -> None:
        """Load persisted config from $HERMES_HOME/talon-recall.json if present."""
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
        self._recency_half_life_days = int(
            cfg.get("recency_half_life_days", self._recency_half_life_days)
        )
        self._fast = bool(cfg.get("fast", self._fast))
        self._prior_message_count = int(
            cfg.get("prior_message_count", self._prior_message_count)
        )

    def _build_command(self, query: str) -> list[str]:
        """Build the talon recall subprocess argv."""
        assert self._binary is not None
        cmd = [
            self._binary,
            "recall",
            query,
            "--format",
            "prompt-xml",
            "--budget-tokens",
            str(self._budget_tokens),
            "--min-confidence",
            str(self._min_confidence),
            "--recency-half-life-days",
            str(self._recency_half_life_days),
        ]

        # Feed the last N user turns as --prior-message args.
        # Only the user side is passed; assistant replies would dilute BM25 signal.
        count = min(self._prior_message_count, len(self._turn_history))
        if count > 0:
            recent = list(self._turn_history)[-count:]
            for user_msg, _ in recent:
                if user_msg.strip():
                    cmd += ["--prior-message", user_msg]

        if self._fast:
            cmd.append("--fast")

        return cmd

    def _build_env(self) -> dict[str, str]:
        """Build subprocess env, injecting TALON_VAULT when vault_path is configured."""
        env = os.environ.copy()
        if self._vault_path:
            env["TALON_VAULT"] = self._vault_path
        return env
