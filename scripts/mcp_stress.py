#!/usr/bin/env python3
"""Drive `talon mcp` over stdio to simulate a long Claude Code session."""

from __future__ import annotations

import argparse
import json
import random
import selectors
import subprocess
import sys
import statistics
import time
from pathlib import Path
from typing import Any


DEFAULT_MESSAGES = [
    "What are the current blockers for the Calle Sur fermented hot sauce launch?",
    "Which co-packer should we choose for the hot sauce line and why?",
    "Summarize Recipe v3 for co-packer handoff: brine, timing, and target texture.",
    "What does the launch readiness note say about open actions and retrieval notes?",
    "How does Hot Sauce Formulation relate to the Fermented Hot Sauce Line project?",
    "What should we order from Salt Marsh Farm for the spring menu?",
    "Find the tasting notes for Fava and Whey and explain the fermentation risk.",
    "What are the equipment and lease constraints for the tasting counter?",
    "Which spring menu dishes depend on ramps, fiddleheads, fava, or watercress?",
    "Non-ASCII vault-ish prompt: Lúcia, Andrés, café, jalapeño, São-style service notes.",
    "Markdown-ish prompt:\n\n```menu\nCharred Spring Onion\nFava and Whey\nLamb Neck\n```\n\nWhich notes should I check before service?",
    "Path-ish prompt: projects/Fermented Hot Sauce Line/Launch Readiness.md and raw/Email - Distributor Quote Hot Sauce Co-Pack.md",
    'Markdown-ish prompt:\n\n```toml\n[inference]\nbase_url = "http://localhost:8000"\n```\n\nWhat should change in Docker?',
]


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Stress-test talon MCP by sending repeated recall hook calls."
    )
    parser.add_argument(
        "--config",
        default=None,
        help="config file to pass with -c; omit to use talon's live default config",
    )
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
    parser.add_argument(
        "--hook-fast",
        action="store_true",
        help="send fast=true to talon_hook_recall for lexical-only hook profiling",
    )
    args = parser.parse_args()
    rng = random.Random(args.seed)

    cmd = ["cargo", "run", "-q", "-p", "talon-cli"]
    if args.release:
        cmd.insert(2, "--release")
    cmd.append("--")
    if args.config is not None:
        cmd.extend(["-c", str(Path(args.config))])
    cmd.append("mcp")

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
        send(
            child,
            {"jsonrpc": "2.0", "id": request_id, "method": "initialize", "params": {}},
        )
        response = read_response(child, selector, args.timeout)
        require_response_id(response, request_id)
        request_id += 1
        send(child, {"jsonrpc": "2.0", "method": "notifications/initialized"})

        started = time.monotonic()
        latencies_ms: list[float] = []
        for turn in range(1, args.turns + 1):
            message = message_for_turn(turn, rng)
            turn_started = time.monotonic()
            response = call_recall(
                child,
                selector,
                request_id,
                turn,
                message,
                args.timeout,
                args.hook_fast,
            )
            latency_ms = (time.monotonic() - turn_started) * 1000
            latencies_ms.append(latency_ms)
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
                print(
                    f"turn {turn}/{args.turns} ok "
                    f"({elapsed:.1f}s, last={latency_ms:.1f}ms)"
                )

        send(child, {"jsonrpc": "2.0", "id": request_id, "method": "shutdown"})
        require_response_id(read_response(child, selector, args.timeout), request_id)
        child.stdin.close()
        code = child.wait(timeout=args.timeout)
        if code != 0:
            fail(f"talon mcp exited with status {code}: {child.stderr.read()}")
        print(f"mcp stress passed: {args.turns} recall turns")
        print(latency_summary(latencies_ms))
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
    fast: bool,
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
                    "fast": fast,
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
        "Repeated suppression probe: recall the hot sauce launch blockers, "
        "co-packer recommendation, and Recipe v3 handoff notes. "
        f"(turn marker {turn % 3})"
    )


def noisy_prompt(turn: int, rng: random.Random) -> str:
    fragments = [
        "paths: projects/Fermented Hot Sauce Line/Co-Packer Research.md",
        'json: {"topic":"hot sauce","scope":"projects","ok":true}',
        "unicode: Lúcia Andrés café jalapeño São 東京",
        "operators: && || $(pwd) `date` [[Launch Readiness]] #hot-sauce",
        "quotes: 'Artisan Ferments Co' \"Coastal Artisan Foods\" [Salt Marsh] (Maria)",
    ]
    rng.shuffle(fragments)
    return f"noisy turn {turn}\n" + "\n".join(fragments)


def long_prompt(turn: int) -> str:
    paragraph = (
        "We are planning the next Calle Sur operating pass. Connect the fermented "
        "hot sauce launch checklist, Recipe v3, co-packer research, Salt Marsh "
        "spring sourcing, and the Spring 2026 menu. Pull the notes that clarify "
        "current blockers, production risks, service prep, and financial tradeoffs. "
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
        stderr = (
            child.stderr.read()
            if child.stderr is not None and child.poll() is not None
            else ""
        )
        fail(
            f"timed out waiting for MCP response; status={child.poll()} stderr={stderr}"
        )
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


def latency_summary(latencies_ms: list[float]) -> str:
    if not latencies_ms:
        return "latency: no turns recorded"
    ordered = sorted(latencies_ms)
    return (
        "latency ms: "
        f"min={ordered[0]:.1f} "
        f"p50={percentile(ordered, 50):.1f} "
        f"p95={percentile(ordered, 95):.1f} "
        f"max={ordered[-1]:.1f} "
        f"mean={statistics.fmean(ordered):.1f}"
    )


def percentile(ordered: list[float], pct: int) -> float:
    if len(ordered) == 1:
        return ordered[0]
    rank = (len(ordered) - 1) * (pct / 100)
    lower = int(rank)
    upper = min(lower + 1, len(ordered) - 1)
    weight = rank - lower
    return ordered[lower] * (1 - weight) + ordered[upper] * weight


def fail(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


if __name__ == "__main__":
    raise SystemExit(main())
