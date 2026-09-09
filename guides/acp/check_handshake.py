#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Bounded, no-model ACP Lab 01 probes. Python 3.9+, standard library only."""
import argparse
import datetime
import json
import os
from pathlib import Path
import subprocess
import tempfile


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def initialize(version=1, request_id: object = 1, capabilities=None):
    return {"jsonrpc": "2.0", "id": request_id, "method": "initialize",
            "params": {"protocolVersion": version,
                       "clientCapabilities": capabilities or {}}}


def exchange(command, frames, timeout):
    env = os.environ.copy()
    for name in ("OPENROUTER_API_KEY", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"):
        env.pop(name, None)
    # No project .env in this launch directory; handshake needs no credentials.
    with tempfile.TemporaryDirectory(prefix="acp-probe-cwd-") as cwd:
        proc = subprocess.Popen(command, stdin=subprocess.PIPE,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                text=True, cwd=cwd, env=env)
        try:
            stdout, stderr = proc.communicate(
                "".join(json.dumps(frame) + "\n" for frame in frames), timeout=timeout)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.communicate()
            raise AssertionError("process did not finish after EOF before timeout")
    require(proc.returncode == 0, "nonzero exit: " + str(proc.returncode))
    require(not stdout or stdout.endswith("\n"), "stdout frame missing newline")
    decoded = [json.loads(line) for line in stdout.splitlines()]
    require(all(isinstance(frame, dict) for frame in decoded), "non-object JSON frame")
    require(all(frame.get("jsonrpc") == "2.0" for frame in decoded), "bad JSON-RPC version")
    return decoded, len(stderr.encode())


def check_initialize(frames, expected_id):
    require(len(frames) == 1, "expected exactly one initialization response")
    frame = frames[0]
    require(frame.get("id") == expected_id and type(frame.get("id")) is type(expected_id),
            "request ID/value/type mismatch")
    require("error" not in frame, "initialization returned an error")
    result = frame["result"]
    require(type(result.get("protocolVersion")) is int and result["protocolVersion"] == 1,
            "expected selected protocol version 1")
    caps = result["agentCapabilities"]
    require(isinstance(caps, dict), "capabilities must be an object")
    require(not caps.get("loadSession", False), "lab must not promise session loading")
    for section, keys in (("promptCapabilities", ("image", "audio", "embeddedContext")),
                          ("mcpCapabilities", ("http", "sse"))):
        require(not any(caps.get(section, {}).get(key, False) for key in keys),
                "lab promises an unimplemented optional capability")
    require(result.get("authMethods", []) == [], "handshake lab must not require authentication")


def run_checks(command, timeout):
    cases = [
        ("initialize_v1", [initialize()], 1),
        ("preserve_string_request_id", [initialize(request_id="init-A")], "init-A"),
        ("unsupported_version_selects_v1", [initialize(version=2)], 1),
        ("client_capabilities_do_not_become_agent_promises",
         [initialize(capabilities={"fs": {"readTextFile": True, "writeTextFile": True},
                                   "terminal": True})], 1),
        ("unknown_method_is_correlated_error",
         [initialize(), {"jsonrpc": "2.0", "id": 2, "method": "lab/not-a-method", "params": {}}], None),
        ("empty_stdin_exits_cleanly", [], None),
    ]
    results = []
    for name, requests, expected_id in cases:
        try:
            frames, stderr_bytes = exchange(command, requests, timeout)
            if expected_id is not None:
                check_initialize(frames, expected_id)
            elif requests:
                require(len(frames) == 2, "expected init response and unknown-method response")
                check_initialize([frames[0]], 1)
                error = frames[1]
                require(error.get("id") == 2 and "result" not in error, "bad error correlation")
                require(error["error"]["code"] == -32601, "expected Method not found (-32601)")
            else:
                require(frames == [], "empty input unexpectedly emitted protocol messages")
            results.append({"name": name, "passed": True, "frames": frames,
                            "stderr_bytes": stderr_bytes})
        except (AssertionError, OSError, ValueError, KeyError, TypeError) as exc:
            results.append({"name": name, "passed": False, "failure": str(exc)})
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, help="write actual results as JSON (overwrites)")
    parser.add_argument("--timeout", type=float, default=5.0, help="seconds per fresh process")
    parser.add_argument("--command", nargs=argparse.REMAINDER, required=True,
                        help="absolute executable path followed by its arguments; put last")
    args = parser.parse_args()
    if not args.command or not Path(args.command[0]).is_absolute():
        parser.error("--command must begin with an absolute executable path")
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    results = run_checks(args.command, args.timeout)
    report = {"checked_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "scope": "Lab 01 only; not full ACP conformance or learner mastery",
              "command": args.command, "total": len(results),
              "passed": sum(item["passed"] for item in results), "results": results}
    if args.report:
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] == report["total"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
