#!/usr/bin/env python3
"""Drive `talon mcp` over stdio to simulate a long Claude Code session."""

from __future__ import annotations

import argparse
import json
import random
import selectors
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


DEFAULT_MESSAGES = [
    "fermented hot sauce co-packer",
    "maybe check ~/.config/talon/config.toml what it currently sets raw/ to",
    "Yeah do all of them. And also align the flags with pplx, ddg etc...",
    "what did we decide about MCP hook recall and context overflow?",
    "search for graph intelligence notes and memory retrieval notes",
    "Can you inspect the Calle Sur launch plan, especially blockers around permits?",
    "Follow up on that same graph intelligence question, but only if it is still useful.",
    "raw/ vs processed/ config paths: what did we write down?",
    "Non-ASCII smoke: café, jalapeño, naïve, 東京, emoji-ish text without emoji.",
    "Shell-ish prompt: rg 'talon_hook_recall|mcp-ready' ~/.config/talon/config.toml",
    "Markdown-ish prompt:\n\n```toml\n[inference]\nbase_url = \"http://localhost:8000\"\n```\n\nWhat should change in Docker?",
]


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Stress-test talon MCP by sending repeated recall hook calls."
    )
    parser.add_argument("--config", default="examples/config.toml")
    parser.add_argument("--turns", type=int, default=100)
    parser.add_argument("--timeout", type=float, default=20.0)
    parser.add_argument("--sleep-ms", type=int, default=0)
    parser.add_argument(
        "--jitter-ms",
        type=int,
        default=0,
        help="add deterministic random sleep from 0..N ms between turns",
    )
    parser.add_argument("--seed", type=int, default=20260506)
    parser.add_argument(
        "--release",
        action="store_true",
        help="run the release binary through cargo run --release",
    )
    args = parser.parse_args()
    rng = random.Random(args.seed)

    config = Path(args.config)
    cmd = ["cargo", "run", "-q", "-p", "talon-cli"]
    if args.release:
        cmd.insert(2, "--release")
    cmd.extend(["--", "-c", str(config), "mcp"])

    child = subprocess.Popen(
        cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    selector = selectors.DefaultSelector()
    if child.stdout is None or child.stdin is None or child.stderr is None:
        raise RuntimeError("failed to open child pipes")
    selector.register(child.stdout, selectors.EVENT_READ)

    try:
        request_id = 1
        send(child, {"jsonrpc": "2.0", "id": request_id, "method": "initialize", "params": {}})
        response = read_response(child, selector, args.timeout)
        require_response_id(response, request_id)
        request_id += 1
        send(child, {"jsonrpc": "2.0", "method": "notifications/initialized"})

        started = time.monotonic()
        for turn in range(1, args.turns + 1):
            message = message_for_turn(turn, rng)
            response = call_recall(child, selector, request_id, turn, message, args.timeout)
            require_response_id(response, request_id)
            validate_recall_response(response, turn)
            request_id += 1
            sleep_ms = args.sleep_ms
            if args.jitter_ms > 0:
                sleep_ms += rng.randint(0, args.jitter_ms)
            if sleep_ms > 0:
                time.sleep(sleep_ms / 1000)
            if turn == 1 or turn % 10 == 0:
                elapsed = time.monotonic() - started
                print(f"turn {turn}/{args.turns} ok ({elapsed:.1f}s)")

        send(child, {"jsonrpc": "2.0", "id": request_id, "method": "shutdown"})
        require_response_id(read_response(child, selector, args.timeout), request_id)
        child.stdin.close()
        code = child.wait(timeout=args.timeout)
        if code != 0:
            fail(f"talon mcp exited with status {code}: {child.stderr.read()}")
        print(f"mcp stress passed: {args.turns} recall turns")
        return 0
    finally:
        selector.close()
        if child.poll() is None:
            child.kill()
            child.wait()


def call_recall(
    child: subprocess.Popen[str],
    selector: selectors.BaseSelector,
    request_id: int,
    turn: int,
    message: str,
    timeout: float,
) -> dict[str, Any]:
    send(
        child,
        {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {
                "name": "talon_hook_recall",
                "arguments": {
                    "host": "claude-code",
                    "sessionId": "mcp-stress",
                    "turnId": f"mcp-stress:{turn}",
                    "cwd": str(Path.cwd()),
                    "transcriptPath": "/tmp/talon-mcp-stress.jsonl",
                    "message": message,
                    "budgetTokens": 500,
                    "format": "hook-json",
                },
            },
        },
    )
    return read_response(child, selector, timeout)


def message_for_turn(turn: int, rng: random.Random) -> str:
    if turn % 17 == 0:
        return long_prompt(turn)
    if turn % 13 == 0:
        return repeated_suppression_prompt(turn)
    if turn % 11 == 0:
        return noisy_prompt(turn, rng)
    return DEFAULT_MESSAGES[(turn - 1) % len(DEFAULT_MESSAGES)]


def repeated_suppression_prompt(turn: int) -> str:
    return (
        "Repeated suppression probe: what did we decide about MCP hook recall, "
        "context overflow, and silent crash containment? "
        f"(turn marker {turn % 3})"
    )


def noisy_prompt(turn: int, rng: random.Random) -> str:
    fragments = [
        "paths: ~/Library/Logs/DiagnosticReports/talon-2026-05-05-141304.000.ips",
        "json: {\"tool\":\"talon_hook_recall\",\"ok\":true}",
        "unicode: café jalapeño naïve 東京 русский",
        "operators: && || $(pwd) `date` <vault_recall skipped=\"true\"/>",
        "quotes: 'single' \"double\" [brackets] (parens) #tags/wiki",
    ]
    rng.shuffle(fragments)
    return f"noisy turn {turn}\n" + "\n".join(fragments)


def long_prompt(turn: int) -> str:
    paragraph = (
        "We are debugging a long-running Claude Code MCP session where recall works "
        "for a few turns and then fails silently after a panic. Please reason about "
        "the distinction between the current user prompt, the transcript path, the "
        "session ledger, suppression of duplicate chunks, and sidecar latency. "
    )
    body = "\n".join(f"{i}. {paragraph}" for i in range(1, 18))
    return f"long transcript-like prompt turn {turn}\n\n{body}"


def send(child: subprocess.Popen[str], request: dict[str, Any]) -> None:
    if child.poll() is not None:
        fail(f"talon mcp exited before request: status {child.returncode}")
    assert child.stdin is not None
    child.stdin.write(json.dumps(request, separators=(",", ":")) + "\n")
    child.stdin.flush()


def read_response(
    child: subprocess.Popen[str], selector: selectors.BaseSelector, timeout: float
) -> dict[str, Any]:
    events = selector.select(timeout)
    if not events:
        stderr = child.stderr.read() if child.stderr is not None and child.poll() is not None else ""
        fail(f"timed out waiting for MCP response; status={child.poll()} stderr={stderr}")
    line = events[0][0].fileobj.readline()
    if not line:
        stderr = child.stderr.read() if child.stderr is not None else ""
        fail(f"MCP connection closed; status={child.poll()} stderr={stderr}")
    try:
        response = json.loads(line)
    except json.JSONDecodeError as error:
        fail(f"invalid JSON response: {error}: {line}")
    if not isinstance(response, dict):
        fail(f"MCP response was not an object: {response!r}")
    return response


def require_response_id(response: dict[str, Any], expected: int) -> None:
    if response.get("id") != expected:
        fail(f"expected response id {expected}, got {response}")
    if "error" in response:
        fail(f"MCP returned JSON-RPC error: {response['error']}")


def validate_recall_response(response: dict[str, Any], turn: int) -> None:
    result = response.get("result")
    if not isinstance(result, dict):
        fail(f"turn {turn}: missing result object: {response}")
    if result.get("isError") is True:
        fail(f"turn {turn}: tool returned error: {result}")
    content = result.get("content")
    if not isinstance(content, list) or not content:
        fail(f"turn {turn}: missing content: {response}")
    text = content[0].get("text") if isinstance(content[0], dict) else None
    if not isinstance(text, str):
        fail(f"turn {turn}: missing text content: {response}")
    try:
        hook_payload = json.loads(text)
    except json.JSONDecodeError as error:
        fail(f"turn {turn}: recall text was not JSON: {error}: {text}")
    if "hookSpecificOutput" not in hook_payload:
        fail(f"turn {turn}: missing hookSpecificOutput: {hook_payload}")


def fail(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


if __name__ == "__main__":
    raise SystemExit(main())
