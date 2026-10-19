"""Test ACP initialization through a real subprocess.

The test suite starts the supplied executable once per case, writes newline-delimited
JSON-RPC requests to its stdin, and inspects the JSON lines written to stdout.
It deliberately removes common model API keys and uses a temporary working
directory: ACP initialization should not require a model or a repository cwd.

One probe keeps stdin open until the response arrives. This catches agents that
buffer the initialization response until the client disconnects.

This is a focused initialization test, not an ACP conformance suite.
Python 3.14+.
"""
from abc import ABC, abstractmethod
import argparse
from dataclasses import dataclass
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


@dataclass
class TestResult:
    name: str
    passed: bool
    response_frames: list[dict] | None = None
    stderr_bytes: int | None = None
    failure: str | None = None

    def to_dict(self):
        return {
            key: value
            for key, value in vars(self).items()
            if value is not None
        }


@dataclass
class TestCase(ABC):
    name: str

    @abstractmethod
    def execute(self, agent) -> tuple[list[dict], int]:
        """Run one protocol scenario and return its response frames and stderr size."""
        raise NotImplementedError

    def run(self, executable, timeout):
        try:
            with AgentProcess(executable, timeout) as agent:
                response_frames, stderr_bytes = self.execute(agent)
            return TestResult(
                self.name,
                True,
                response_frames=response_frames,
                stderr_bytes=stderr_bytes,
            )
        except (AssertionError, OSError, ValueError, KeyError, TypeError) as exc:
            return TestResult(self.name, False, failure=str(exc))


@dataclass
class InitializeTestCase(TestCase):
    request: dict
    expected_id: object

    def execute(self, agent):
        agent.send(self.request)
        response_frames, stderr_bytes = agent.finish()
        validate_initialize_response(response_frames, self.expected_id)
        return response_frames, stderr_bytes


class InitializeBeforeEofTestCase(InitializeTestCase):
    def execute(self, agent):
        agent.send(self.request)
        first_frame = agent.read()
        remaining_frames, stderr_bytes = agent.finish()
        response_frames = [first_frame, *remaining_frames]
        validate_initialize_response(response_frames, self.expected_id)
        return response_frames, stderr_bytes


@dataclass
class UnknownMethodTestCase(TestCase):
    request: dict

    def execute(self, agent):
        agent.send(initialize())
        agent.send(self.request)
        response_frames, stderr_bytes = agent.finish()
        require(len(response_frames) == 2,
                "expected init response and unknown-method response")
        validate_initialize_response([response_frames[0]], 1)
        error = response_frames[1]
        require(error.get("id") == self.request["id"] and "result" not in error,
                "bad error correlation")
        require(error["error"]["code"] == -32601,
                "expected Method not found (-32601)")
        return response_frames, stderr_bytes


class EmptyInputTestCase(TestCase):
    def execute(self, agent):
        response_frames, stderr_bytes = agent.finish()
        require(response_frames == [],
                "empty input unexpectedly emitted protocol messages")
        return response_frames, stderr_bytes


@dataclass
class TestSuite:
    cases: list[TestCase]

    def run(self, executable, timeout):
        return [case.run(executable, timeout) for case in self.cases]


def require(condition, message):
    """Raise a readable test failure when an expected condition is false."""
    if not condition:
        raise AssertionError(message)


def environment_without_model_keys():
    """Return a child environment that cannot use common model credentials."""
    env = os.environ.copy()
    for name in MODEL_KEY_VARIABLES:
        env.pop(name, None)
    return env


class AgentProcess:
    """Drive one isolated ACP subprocess through newline-delimited JSON."""

    def __init__(self, executable, timeout):
        self.timeout = timeout
        self._temporary_directory = tempfile.TemporaryDirectory(
            prefix="acp-probe-cwd-"
        )
        self.process = subprocess.Popen(
            executable,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=self._temporary_directory.name,
            env=environment_without_model_keys(),
        )
        assert self.process.stdin is not None
        assert self.process.stdout is not None

    def __enter__(self):
        return self

    def __exit__(self, _exc_type, _exc_value, _traceback):
        if self.process.poll() is None:
            self.process.kill()
            self.process.communicate()
        self._temporary_directory.cleanup()

    def send(self, frame):
        child_stdin = self.process.stdin
        assert child_stdin is not None
        payload = json.dumps(frame).encode() + b"\n"
        child_stdin.write(payload)
        child_stdin.flush()

    def read(self):
        child_stdout = self.process.stdout
        assert child_stdout is not None
        selector = selectors.DefaultSelector()
        try:
            selector.register(child_stdout, selectors.EVENT_READ)
            require(selector.select(self.timeout),
                    "response did not arrive while stdin remained open")
            line = child_stdout.readline()
            require(line, "stdout closed before response")
        finally:
            selector.close()
        return decode_stdout(line)[0]

    def close_stdin(self):
        if self.process.stdin is not None:
            self.process.stdin.close()
            self.process.stdin = None

    def finish(self):
        self.close_stdin()
        try:
            stdout, stderr = self.process.communicate(timeout=self.timeout)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.communicate()
            raise AssertionError("process did not finish after EOF before timeout")
        require(self.process.returncode == 0,
                "nonzero exit: " + str(self.process.returncode))
        return decode_stdout(stdout), len(stderr)


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



def validate_initialize_response(frames, expected_id):
    """Validate the fields and conservative capabilities of one init response."""
    require(len(frames) == 1, "expected exactly one initialization response")

    # Ensure JSON-RPC response shape.
    frame = frames[0]
    require(frame.get("id") == expected_id and type(frame.get("id")) is type(expected_id),
            "request ID/value/type mismatch")
    require("error" not in frame, "initialization returned an error")

    # Ensure we support ACP version 1.
    result = frame["result"]
    require(type(result.get("protocolVersion")) is int and result["protocolVersion"] == 1,
            "expected selected protocol version 1")

    # Validate agentic capabilities.
    caps = result["agentCapabilities"]
    require(isinstance(caps, dict), "capabilities must be an object")
    require(not caps.get("loadSession", False), "agent must not promise session loading")
    for section, keys in (("promptCapabilities", ("image", "audio", "embeddedContext")),
                          ("mcpCapabilities", ("http", "sse"))):
        require(not any(caps.get(section, {}).get(key, False) for key in keys),
                "agent promises an unimplemented optional capability")
    require(result.get("authMethods", []) == [],
            "initialization task must not require authentication")


def run_tests(executable, timeout):
    """Run every initialization scenario and retain failures in one report.

    Cases are collected rather than aborted on the first failure so the report
    shows all observed protocol-boundary failures from one invocation.
    """
    suite = TestSuite([
        InitializeTestCase("initialize_v1", initialize(), 1),
        InitializeTestCase(
            "preserve_string_request_id",
            initialize(request_id="init-A"),
            "init-A",
        ),
        InitializeTestCase(
            "unsupported_version_selects_v1",
            initialize(version=2),
            1,
        ),
        InitializeTestCase(
            "client_capabilities_do_not_become_agent_promises",
            initialize(capabilities={
                "fs": {"readTextFile": True, "writeTextFile": True},
                "terminal": True,
            }),
            1,
        ),
        UnknownMethodTestCase(
            "unknown_method_is_correlated_error",
            {"jsonrpc": "2.0", "id": 2,
             "method": "lab/not-a-method", "params": {}},
        ),
        EmptyInputTestCase("empty_stdin_exits_cleanly"),
        InitializeBeforeEofTestCase(
            "initialize_reply_arrives_before_eof",
            initialize(),
            1,
        ),
    ])
    return suite.run(executable, timeout)


def main():
    """Parse test options, run the cases, and optionally write a JSON report."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path,
                        help="write actual results as JSON (overwrites)")
    parser.add_argument("--timeout", type=float, default=5.0,
                        help="seconds per fresh process")
    parser.add_argument("--executable", nargs=argparse.REMAINDER, required=True,
                        help="executable path followed by its arguments; put last")
    args = parser.parse_args()
    if not args.executable:
        parser.error("--executable must include an executable path")
    args.executable[0] = str(Path(args.executable[0]).expanduser().resolve())
    if args.timeout <= 0:
        parser.error("--timeout must be positive")

    results = run_tests(args.executable, args.timeout)
    report = {
        "tested_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "ACP initialization only; not full conformance",
        "executable": args.executable,
        "total": len(results),
        "passed": sum(result.passed for result in results),
        "results": [result.to_dict() for result in results],
    }
    if args.report:
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] == report["total"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
