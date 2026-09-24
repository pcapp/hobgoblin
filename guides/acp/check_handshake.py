#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Check ACP initialization through a real subprocess.

The checker starts the supplied command once per case, writes newline-delimited
JSON-RPC requests to its stdin, and inspects the JSON lines written to stdout.
It deliberately removes common model API keys and uses a temporary working
directory: ACP initialization should not require a model or a repository cwd.

One probe keeps stdin open until the response arrives. This catches agents that
buffer the initialization response until the client disconnects.

This is a focused initialization check, not an ACP conformance suite.
Python 3.9+; standard library only.
"""
import argparse
import datetime
import json
import os
from pathlib import Path
import selectors
import subprocess
import tempfile


MODEL_KEY_VARIABLES = (
    "OPENROUTER_API_KEY",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
)


def require(condition, message):
    """Raise a readable check failure when an expected condition is false."""
    if not condition:
        raise AssertionError(message)


def environment_without_model_keys():
    """Return a child environment that cannot use common model credentials."""
    env = os.environ.copy()
    for name in MODEL_KEY_VARIABLES:
        env.pop(name, None)
    return env


def initialize(version=1, request_id: object = 1, capabilities=None):
    """Build one ACP initialize request with caller-controlled test values."""
    return {
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "initialize",
        "params": {
            "protocolVersion": version,
            "clientCapabilities": capabilities or {},
        },
    }


def decode_stdout(stdout):
    """Decode newline-terminated JSON-RPC objects emitted by the child."""
    require(not stdout or stdout.endswith(b"\n"), "stdout frame missing newline")
    decoded = [json.loads(line) for line in stdout.splitlines()]
    require(all(isinstance(frame, dict) for frame in decoded), "non-object JSON frame")
    require(all(frame.get("jsonrpc") == "2.0" for frame in decoded),
            "bad JSON-RPC version")
    return decoded


def exchange(command, frames, timeout):
    """Run one isolated exchange, closing stdin after writing all requests.

    A fresh process prevents one case's initialization state from affecting the
    next. Returning the stderr byte count records diagnostic activity without
    copying potentially sensitive diagnostic text into the report.
    """
    payload = b"".join(json.dumps(frame).encode() + b"\n" for frame in frames)
    with tempfile.TemporaryDirectory(prefix="acp-probe-cwd-") as cwd:
        proc = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=cwd,
            env=environment_without_model_keys(),
        )
        try:
            stdout, stderr = proc.communicate(payload, timeout=timeout)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.communicate()
            raise AssertionError("process did not finish after EOF before timeout")
    require(proc.returncode == 0, "nonzero exit: " + str(proc.returncode))
    return decode_stdout(stdout), len(stderr)


def exchange_before_eof(command, frame, timeout):
    """Read one response while the child's stdin remains open.

    ``selectors`` supplies a deadline without starting a reader thread. After
    the first response arrives, the function closes stdin and requires the child
    to terminate cleanly.
    """
    payload = json.dumps(frame).encode() + b"\n"
    with tempfile.TemporaryDirectory(prefix="acp-probe-cwd-") as cwd:
        proc = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=cwd,
            env=environment_without_model_keys(),
        )
        child_stdin = proc.stdin
        child_stdout = proc.stdout
        assert child_stdin is not None
        assert child_stdout is not None

        selector = selectors.DefaultSelector()
        try:
            child_stdin.write(payload)
            child_stdin.flush()
            selector.register(child_stdout, selectors.EVENT_READ)
            require(selector.select(timeout),
                    "initialization response did not arrive while stdin remained open")
            first_line = child_stdout.readline()
            require(first_line, "stdout closed before initialization response")

            child_stdin.close()
            proc.stdin = None
            try:
                remaining_stdout, stderr = proc.communicate(timeout=timeout)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.communicate()
                raise AssertionError("process did not finish after EOF before timeout")
        finally:
            selector.close()
            if proc.poll() is None:
                proc.kill()
                proc.communicate()

    require(proc.returncode == 0, "nonzero exit: " + str(proc.returncode))
    return decode_stdout(first_line + remaining_stdout), len(stderr)


def check_initialize(frames, expected_id):
    """Validate the fields and conservative capabilities of one init response."""
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
    require(not caps.get("loadSession", False), "agent must not promise session loading")
    for section, keys in (("promptCapabilities", ("image", "audio", "embeddedContext")),
                          ("mcpCapabilities", ("http", "sse"))):
        require(not any(caps.get(section, {}).get(key, False) for key in keys),
                "agent promises an unimplemented optional capability")
    require(result.get("authMethods", []) == [],
            "initialization task must not require authentication")


def run_checks(command, timeout):
    """Run every initialization scenario and retain failures in one report.

    Cases are collected rather than aborted on the first failure so the report
    shows all observed protocol-boundary failures from one invocation.
    """
    cases = [
        ("initialize_v1", [initialize()], 1),
        ("preserve_string_request_id", [initialize(request_id="init-A")], "init-A"),
        ("unsupported_version_selects_v1", [initialize(version=2)], 1),
        ("client_capabilities_do_not_become_agent_promises",
         [initialize(capabilities={"fs": {"readTextFile": True, "writeTextFile": True},
                                   "terminal": True})], 1),
        ("unknown_method_is_correlated_error",
         [initialize(), {"jsonrpc": "2.0", "id": 2,
                         "method": "lab/not-a-method", "params": {}}], None),
        ("empty_stdin_exits_cleanly", [], None),
    ]
    results = []
    for name, requests, expected_id in cases:
        try:
            frames, stderr_bytes = exchange(command, requests, timeout)
            if expected_id is not None:
                check_initialize(frames, expected_id)
            elif requests:
                require(len(frames) == 2,
                        "expected init response and unknown-method response")
                check_initialize([frames[0]], 1)
                error = frames[1]
                require(error.get("id") == 2 and "result" not in error,
                        "bad error correlation")
                require(error["error"]["code"] == -32601,
                        "expected Method not found (-32601)")
            else:
                require(frames == [], "empty input unexpectedly emitted protocol messages")
            results.append({"name": name, "passed": True, "frames": frames,
                            "stderr_bytes": stderr_bytes})
        except (AssertionError, OSError, ValueError, KeyError, TypeError) as exc:
            results.append({"name": name, "passed": False, "failure": str(exc)})

    name = "initialize_reply_arrives_before_eof"
    try:
        frames, stderr_bytes = exchange_before_eof(command, initialize(), timeout)
        check_initialize(frames, 1)
        results.append({"name": name, "passed": True, "frames": frames,
                        "stderr_bytes": stderr_bytes})
    except (AssertionError, OSError, ValueError, KeyError, TypeError) as exc:
        results.append({"name": name, "passed": False, "failure": str(exc)})

    return results


def main():
    """Parse checker options, run the probes, and optionally write a JSON report."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path,
                        help="write actual results as JSON (overwrites)")
    parser.add_argument("--timeout", type=float, default=5.0,
                        help="seconds per fresh process")
    parser.add_argument("--command", nargs=argparse.REMAINDER, required=True,
                        help="absolute executable path followed by its arguments; put last")
    args = parser.parse_args()
    if not args.command or not Path(args.command[0]).is_absolute():
        parser.error("--command must begin with an absolute executable path")
    if args.timeout <= 0:
        parser.error("--timeout must be positive")

    results = run_checks(args.command, args.timeout)
    report = {
        "checked_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "ACP initialization only; not full conformance",
        "command": args.command,
        "total": len(results),
        "passed": sum(item["passed"] for item in results),
        "results": results,
    }
    if args.report:
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] == report["total"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
