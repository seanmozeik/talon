"""TalonRecallProvider: Hermes MemoryProvider backed by `talon mcp`.

Uses a persistent MCP child process for session-aware, deduplicated vault recall.
Falls back to empty context (not an error) if the MCP process is unavailable.
"""

from __future__ import annotations

import json
import logging
import os
import shutil
import subprocess
import threading
import uuid
from pathlib import Path
from typing import Any

from agent.memory_provider import MemoryProvider

logger = logging.getLogger(__name__)

_NO_RECALL = ""
_SKIPPED_PREFIX = '<vault_recall skipped="true"'
_TIMEOUT = 5.0  # seconds per RPC call
_MCP_INIT_TIMEOUT = 10.0


class TalonMcpClient:
    """Minimal JSON-RPC 2.0 client over a talon mcp stdio child process."""

    def __init__(self, binary: str, env: dict[str, str]) -> None:
        self._binary = binary
        self._env = env
        self._proc: subprocess.Popen | None = None
        self._lock = threading.Lock()
        self._next_id = 1

    def start(self) -> None:
        """Start talon mcp child process and perform MCP handshake."""
        self._proc = subprocess.Popen(
            [self._binary, "mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            env=self._env,
        )
        self._send_init()

    def _send_init(self) -> None:
        """Send MCP initialize + initialized notification."""
        self._rpc_call(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "hermes-talon", "version": "1.0"},
            },
            timeout=_MCP_INIT_TIMEOUT,
        )
        self._send_notification("notifications/initialized")

    def call_tool(
        self, name: str, arguments: dict[str, Any], timeout: float = _TIMEOUT
    ) -> dict[str, Any] | None:
        """Call an MCP tool and return the result dict, or None on failure."""
        result = self._rpc_call(
            "tools/call", {"name": name, "arguments": arguments}, timeout=timeout
        )
        if result is None:
            return None
        return result.get("result")

    def _rpc_call(
        self, method: str, params: dict[str, Any], timeout: float = _TIMEOUT
    ) -> dict[str, Any] | None:
        if self._proc is None or self._proc.poll() is not None:
            return None
        with self._lock:
            req_id = self._next_id
            self._next_id += 1
            request = json.dumps(
                {"jsonrpc": "2.0", "id": req_id, "method": method, "params": params}
            )
            try:
                assert self._proc.stdin is not None
                self._proc.stdin.write((request + "\n").encode())
                self._proc.stdin.flush()
                assert self._proc.stdout is not None
                line = self._proc.stdout.readline()  # blocking read
                if not line:
                    return None
                return json.loads(line.decode())
            except Exception as exc:
                logger.warning("talon-mcp: RPC error for %s: %s", method, exc)
                return None

    def _send_notification(self, method: str) -> None:
        if self._proc is None:
            return
        notification = json.dumps({"jsonrpc": "2.0", "method": method})
        try:
            assert self._proc.stdin is not None
            self._proc.stdin.write((notification + "\n").encode())
            self._proc.stdin.flush()
        except Exception:
            pass

    def shutdown(self) -> None:
        if self._proc is None:
            return
        try:
            self._send_notification("shutdown")
            if self._proc.stdin:
                self._proc.stdin.close()
            self._proc.wait(timeout=3)
        except Exception:
            self._proc.kill()
        self._proc = None

    @property
    def alive(self) -> bool:
        return self._proc is not None and self._proc.poll() is None


class TalonRecallProvider(MemoryProvider):
    """Hermes MemoryProvider — vault-native auto-recall via talon mcp."""

    @property
    def name(self) -> str:
        return "talon-recall"

    def __init__(self) -> None:
        self._binary: str | None = None
        self._vault_path: str | None = None
        self._budget_tokens: int = 500
        self._min_confidence: float = 0.4
        self._fast: bool = False
        self._session_id: str = ""
        self._client: TalonMcpClient | None = None

    # ── MemoryProvider ABC ─────────────────────────────────────────────────

    def is_available(self) -> bool:
        return self._resolve_binary() is not None

    def initialize(self, session_id: str, **kwargs: Any) -> None:
        binary = self._resolve_binary()
        if binary is None:
            return
        self._binary = binary
        self._load_config(kwargs.get("hermes_home", ""))
        if vault_env := os.environ.get("TALON_VAULT"):
            self._vault_path = vault_env

        self._session_id = session_id or str(uuid.uuid4())
        client = TalonMcpClient(binary, self._build_env())
        try:
            client.start()
            client.call_tool(
                "talon_hook_session_start",
                {
                    "host": "hermes",
                    "sessionId": self._session_id,
                },
            )
            self._client = client
        except Exception as exc:
            logger.warning("talon-mcp: failed to start: %s", exc)

    def system_prompt_block(self) -> str:
        return (
            "# Talon Vault\n"
            "Relevant vault notes are injected as <vault_recall> context before each turn. "
            "Duplicate context from prior turns is suppressed automatically. "
            "Use talon_search, talon_read, or talon_related for explicit vault queries."
        )

    def prefetch(self, query: str, *, session_id: str = "") -> str:
        if self._client is None or not self._client.alive:
            return _NO_RECALL
        result = self._client.call_tool(
            "talon_hook_recall",
            {
                "host": "hermes",
                "sessionId": self._session_id,
                "turnId": str(uuid.uuid4()),
                "message": query,
                "budgetTokens": self._budget_tokens,
                "format": "prompt-xml",
            },
        )
        if result is None:
            return _NO_RECALL
        content = result.get("content", [])
        if not content:
            return _NO_RECALL
        text = content[0].get("text", "")
        if not text or text.startswith(_SKIPPED_PREFIX):
            return _NO_RECALL
        return text

    def sync_turn(
        self, user_content: str, assistant_content: str, *, session_id: str = ""
    ) -> None:
        if self._client is None or not self._client.alive:
            return
        self._client.call_tool(
            "talon_hook_turn_end",
            {
                "host": "hermes",
                "sessionId": self._session_id,
                "turnId": str(uuid.uuid4()),
                "outcome": "completed",
                "lastUserMessage": user_content,
                "lastAssistantMessage": assistant_content,
            },
        )

    def get_tool_schemas(self) -> list[dict[str, Any]]:
        return []

    def shutdown(self) -> None:
        if self._client is not None:
            try:
                self._client.call_tool(
                    "talon_hook_session_end",
                    {
                        "host": "hermes",
                        "sessionId": self._session_id,
                    },
                )
            except Exception:
                pass
            self._client.shutdown()
            self._client = None

    # ── config ─────────────────────────────────────────────────────────────

    def get_config_schema(self) -> list[dict[str, Any]]:
        return [
            {
                "key": "vault_path",
                "description": "Absolute path to your Obsidian vault",
                "required": False,
                "env_var": "TALON_VAULT",
            },
            {
                "key": "budget_tokens",
                "description": "Token budget for recall context (default 500)",
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
        ]

    def save_config(self, values: dict[str, Any], hermes_home: str) -> None:
        Path(hermes_home).joinpath("talon-recall.json").write_text(
            json.dumps(values, indent=2)
        )

    # ── private helpers ────────────────────────────────────────────────────

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

    def _build_env(self) -> dict[str, str]:
        env = os.environ.copy()
        if hermes_home := os.environ.get("HERMES_HOME"):
            profile_home = Path(hermes_home) / "home"
            if profile_home.is_dir():
                env["HOME"] = str(profile_home)
                env.setdefault(
                    "TALON_CONFIG_FILE",
                    str(profile_home / ".config" / "talon" / "config.toml"),
                )
        if self._vault_path:
            env["TALON_VAULT"] = self._vault_path
        return env


def register(ctx) -> None:
    register_memory_provider = getattr(ctx, "register_memory_provider", None)
    if register_memory_provider is not None:
        register_memory_provider(TalonRecallProvider())
