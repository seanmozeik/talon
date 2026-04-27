"""Unit tests for TalonRecallProvider.

All tests mock subprocess.run so no live talon binary or Hermes install is required.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import types
from pathlib import Path
from subprocess import CompletedProcess
from unittest.mock import MagicMock, patch

import pytest

# ---------------------------------------------------------------------------
# Stub the agent.memory_provider module so tests run without a Hermes install.
# ---------------------------------------------------------------------------
import abc

_stub_module = types.ModuleType("agent.memory_provider")

class _MemoryProviderStub(abc.ABC):
    pass

_stub_module.MemoryProvider = _MemoryProviderStub
_agent_pkg = types.ModuleType("agent")
sys.modules.setdefault("agent", _agent_pkg)
sys.modules.setdefault("agent.memory_provider", _stub_module)

from hermes_talon_recall.provider import TalonRecallProvider  # noqa: E402

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

GOOD_XML = """\
<vault_recall source="talon" vault="/vault" evidence_score="0.8500">
  <active_notes>
    <note path="Notes/Foo.md" title="Foo" score="0.9120">A relevant snippet.</note>
  </active_notes>
  <linked_context/>
  <frontmatter/>
  <recent_edits/>
  <fuzzy_anchors/>
</vault_recall>"""

SKIPPED_XML = '<vault_recall skipped="true" evidence_score="0.1200"/>'


def _make_provider(monkeypatch, *, binary: str = "/usr/bin/talon") -> TalonRecallProvider:
    """Return an initialized provider with a fake binary path."""
    p = TalonRecallProvider()
    monkeypatch.setattr(
        "hermes_talon_recall.provider.shutil.which", lambda _name: binary
    )
    p.initialize(session_id="test-session")
    return p


# ---------------------------------------------------------------------------
# is_available
# ---------------------------------------------------------------------------


def test_is_available_binary_on_path(monkeypatch):
    monkeypatch.setattr("hermes_talon_recall.provider.shutil.which", lambda _: "/usr/bin/talon")
    p = TalonRecallProvider()
    assert p.is_available() is True


def test_is_available_binary_missing(monkeypatch):
    monkeypatch.setattr("hermes_talon_recall.provider.shutil.which", lambda _: None)
    monkeypatch.delenv("TALON_BIN", raising=False)
    p = TalonRecallProvider()
    assert p.is_available() is False


def test_is_available_via_talon_bin_env(monkeypatch, tmp_path):
    fake_bin = tmp_path / "talon"
    fake_bin.touch()
    fake_bin.chmod(0o755)
    monkeypatch.setenv("TALON_BIN", str(fake_bin))
    monkeypatch.setattr("hermes_talon_recall.provider.shutil.which", lambda _: None)
    p = TalonRecallProvider()
    assert p.is_available() is True


def test_initialize_raises_when_binary_missing(monkeypatch):
    monkeypatch.setattr("hermes_talon_recall.provider.shutil.which", lambda _: None)
    monkeypatch.delenv("TALON_BIN", raising=False)
    p = TalonRecallProvider()
    with pytest.raises(RuntimeError, match="talon binary not found"):
        p.initialize(session_id="x")


# ---------------------------------------------------------------------------
# prefetch: good XML
# ---------------------------------------------------------------------------


def test_prefetch_good_xml(monkeypatch):
    """Happy path: talon returns well-formed vault_recall XML."""
    p = _make_provider(monkeypatch)

    mock_result = CompletedProcess(args=[], returncode=0, stdout=GOOD_XML, stderr="")
    with patch("hermes_talon_recall.provider.subprocess.run", return_value=mock_result):
        result = p.prefetch("What are my notes on Foo?")

    assert result == GOOD_XML


# ---------------------------------------------------------------------------
# prefetch: skipped=true confidence gate
# ---------------------------------------------------------------------------


def test_prefetch_skipped_returns_empty(monkeypatch):
    """When talon returns skipped=true, prefetch returns '' (no-recall)."""
    p = _make_provider(monkeypatch)

    mock_result = CompletedProcess(args=[], returncode=0, stdout=SKIPPED_XML, stderr="")
    with patch("hermes_talon_recall.provider.subprocess.run", return_value=mock_result):
        result = p.prefetch("obscure query with no vault match")

    assert result == ""


# ---------------------------------------------------------------------------
# prefetch: subprocess error
# ---------------------------------------------------------------------------


def test_prefetch_subprocess_exception_returns_empty(monkeypatch):
    """When subprocess.run raises (e.g. OSError), prefetch returns '' silently."""
    p = _make_provider(monkeypatch)

    with patch(
        "hermes_talon_recall.provider.subprocess.run",
        side_effect=OSError("binary not executable"),
    ):
        result = p.prefetch("some query")

    assert result == ""


def test_prefetch_timeout_returns_empty(monkeypatch):
    """When talon takes longer than 20s, prefetch returns '' without raising."""
    p = _make_provider(monkeypatch)

    with patch(
        "hermes_talon_recall.provider.subprocess.run",
        side_effect=subprocess.TimeoutExpired(cmd=["talon"], timeout=20),
    ):
        result = p.prefetch("slow query")

    assert result == ""


# ---------------------------------------------------------------------------
# prefetch: empty stdout
# ---------------------------------------------------------------------------


def test_prefetch_empty_stdout_returns_empty(monkeypatch):
    """When talon produces no output, prefetch returns ''."""
    p = _make_provider(monkeypatch)

    mock_result = CompletedProcess(args=[], returncode=0, stdout="", stderr="")
    with patch("hermes_talon_recall.provider.subprocess.run", return_value=mock_result):
        result = p.prefetch("some query")

    assert result == ""


# ---------------------------------------------------------------------------
# prefetch: non-zero exit code
# ---------------------------------------------------------------------------


def test_prefetch_nonzero_exit_returns_empty(monkeypatch):
    """When talon exits non-zero (config missing, DB error, etc.), prefetch returns ''."""
    p = _make_provider(monkeypatch)

    mock_result = CompletedProcess(
        args=[], returncode=1, stdout="", stderr="config not found"
    )
    with patch("hermes_talon_recall.provider.subprocess.run", return_value=mock_result):
        result = p.prefetch("some query")

    assert result == ""


# ---------------------------------------------------------------------------
# sync_turn populates prior-message buffer
# ---------------------------------------------------------------------------


def test_prior_messages_passed_to_talon(monkeypatch):
    """sync_turn history feeds --prior-message flags on the next prefetch."""
    p = _make_provider(monkeypatch)
    p._prior_message_count = 2

    p.sync_turn("What is a knowledge graph?", "It's a graph of concepts…")
    p.sync_turn("Tell me more about Obsidian.", "Obsidian is a Markdown editor…")

    captured: list[list[str]] = []

    def fake_run(cmd, **_kwargs):
        captured.append(cmd)
        return CompletedProcess(args=[], returncode=0, stdout=GOOD_XML, stderr="")

    with patch("hermes_talon_recall.provider.subprocess.run", fake_run):
        p.prefetch("How do I build a vault?")

    assert len(captured) == 1
    cmd = captured[0]
    prior_indices = [i for i, token in enumerate(cmd) if token == "--prior-message"]
    assert len(prior_indices) == 2
    assert cmd[prior_indices[0] + 1] == "What is a knowledge graph?"
    assert cmd[prior_indices[1] + 1] == "Tell me more about Obsidian."


# ---------------------------------------------------------------------------
# --fast flag
# ---------------------------------------------------------------------------


def test_fast_flag_appended_when_configured(monkeypatch):
    """When fast=True is set, --fast is appended to the talon command."""
    p = _make_provider(monkeypatch)
    p._fast = True

    captured: list[list[str]] = []

    def fake_run(cmd, **_kwargs):
        captured.append(cmd)
        return CompletedProcess(args=[], returncode=0, stdout=GOOD_XML, stderr="")

    with patch("hermes_talon_recall.provider.subprocess.run", fake_run):
        p.prefetch("quick query")

    assert "--fast" in captured[0]


def test_fast_flag_absent_by_default(monkeypatch):
    """By default (fast=False), --fast is NOT in the talon command."""
    p = _make_provider(monkeypatch)

    captured: list[list[str]] = []

    def fake_run(cmd, **_kwargs):
        captured.append(cmd)
        return CompletedProcess(args=[], returncode=0, stdout=GOOD_XML, stderr="")

    with patch("hermes_talon_recall.provider.subprocess.run", fake_run):
        p.prefetch("normal query")

    assert "--fast" not in captured[0]


# ---------------------------------------------------------------------------
# vault_path env var injection
# ---------------------------------------------------------------------------


def test_vault_path_sets_talon_vault_env(monkeypatch):
    """When vault_path is configured, TALON_VAULT is set in subprocess env."""
    p = _make_provider(monkeypatch)
    p._vault_path = "/home/user/vault"

    captured_envs: list[dict] = []

    def fake_run(cmd, *, env=None, **_kwargs):
        captured_envs.append(env or {})
        return CompletedProcess(args=[], returncode=0, stdout=GOOD_XML, stderr="")

    with patch("hermes_talon_recall.provider.subprocess.run", fake_run):
        p.prefetch("vault query")

    assert captured_envs[0].get("TALON_VAULT") == "/home/user/vault"


# ---------------------------------------------------------------------------
# get_tool_schemas
# ---------------------------------------------------------------------------


def test_get_tool_schemas_returns_empty_list():
    """Talon is recall-only; the provider never exposes agent tools."""
    p = TalonRecallProvider()
    assert p.get_tool_schemas() == []


# ---------------------------------------------------------------------------
# save_config / load_config round-trip
# ---------------------------------------------------------------------------


def test_save_and_load_config(tmp_path, monkeypatch):
    """Config persisted by save_config is loaded back in initialize."""
    hermes_home = str(tmp_path)
    values = {
        "vault_path": "/my/vault",
        "budget_tokens": 1500,
        "min_confidence": 0.5,
        "fast": True,
        "prior_message_count": 3,
    }

    writer = TalonRecallProvider()
    writer.save_config(values, hermes_home)

    reader = TalonRecallProvider()
    monkeypatch.setattr("hermes_talon_recall.provider.shutil.which", lambda _: "/usr/bin/talon")
    reader.initialize(session_id="x", hermes_home=hermes_home)

    assert reader._vault_path == "/my/vault"
    assert reader._budget_tokens == 1500
    assert reader._min_confidence == 0.5
    assert reader._fast is True
    assert reader._prior_message_count == 3
