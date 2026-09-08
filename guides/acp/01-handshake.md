# Lab 01 — A typed ACP handshake, with no model

**Scope:** a deliberately incomplete protocol exercise. It only handles initialization; no sessions, model requests, tools, or credentials. Do not present it as a usable or fully conformant ACP agent.

## Your milestone and stopping point

**User-visible result:** a process starts without a model key, reads an `initialize` frame, and sends a valid, correlated v1 reply. Zed can discover its protocol; it cannot converse with it yet.

**You should be able to explain:** process startup versus connection initialization versus session creation; the two directions of capabilities; why stdout belongs to the SDK; what a responder owns.

**You will change:** `Cargo.toml`, the generated `Cargo.lock`, and your new `examples/acp_handshake.rs`. Leave `src/` alone for this milestone. Hermes has supplied a review script, not the Rust solution in your repository.

Read the wiki's [initialization mental model](file:///Users/peter/knowledge-bundles/harness-engineering/concepts/acp-initialization.md) first. Stop after the review gate below; do not add a model or tools today.

## Concept first

The SDK connects typed Rust callbacks to JSON-RPC methods. `InitializeRequest` lets the compiler check your field access. `Responder` sends the corresponding reply. `Agent.builder()` configures the role and callbacks; `Stdio::new()` owns wire I/O. Tokio drives the connection future.

The code returns the latest protocol version it actually supports: v1. It does not blindly echo an unsupported incoming version. Empty optional capabilities make no extra feature promises, but they do not waive baseline session/content/MCP requirements for a finished agent.

Sources: [initialization](https://agentclientprotocol.com/protocol/v1/initialization), [pinned cookbook](https://docs.rs/agent-client-protocol-cookbook/2.1.0/agent_client_protocol_cookbook/building_an_agent/), [SDK](https://docs.rs/agent-client-protocol/2.1.0/agent_client_protocol/).

## Before editing

Complete [Lab 00](00-inspect.md) and run its baseline checks. In `Cargo.toml:8–17`, confirm there is no direct ACP dependency. Review `git diff` so you know which changes are yours.

## 0. Make the contract executable first

From `/Users/peter/repos/codecrafters-claude-code-rust`, run:

```sh
python3 guides/acp/check_handshake.py --command "$PWD/target/debug/examples/acp_handshake"
```

`$PWD` expands to the repository's absolute path. `--command` must be last; all following arguments belong to the child executable. The supplied verifier uses Python's standard library, fresh processes and a five-second timeout per case. It strips common model-key environment variables and uses a temporary cwd to avoid loading the repo's `.env`. It is a test runner, **not a security sandbox**; only run the no-model lab with it.

Before you create/build the example, expect nonzero exit with missing-executable failures. This establishes the harness is wired up; it is not yet a behavioral red test. For a behavioral red/green exercise after the first build, use the controlled stdout mutation below.

## 1. Add one dependency yourself

Under `[dependencies]` in `Cargo.toml:8`, type:

```toml
agent-client-protocol = "=2.1.0"
```

**Rationale:** pin the API taught here. The leading `=` is an exact version constraint; it is not the ACP wire version. Keep the existing Tokio dependency and avoid unstable features.

## 2. Create a separate example yourself

Create the new file `examples/acp_handshake.rs:1` and type:

```rust
use agent_client_protocol::{Agent, Result, Stdio};
use agent_client_protocol::schema::{
    ProtocolVersion,
    v1::{AgentCapabilities, InitializeRequest, InitializeResponse},
};

#[tokio::main]
async fn main() -> Result<()> {
    Agent.builder()
        .name("acp-handshake-lab")
        .on_receive_request(
            async |_request: InitializeRequest, responder, _connection| {
                responder.respond(
                    InitializeResponse::new(ProtocolVersion::V1)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(Stdio::new())
        .await
}

```

**Rationale:** a Cargo example isolates the protocol lesson from the current CLI and provider setup. No edit to `src/` is needed for this lab.

`_request` and `_connection` indicate intentionally unused values. In the full adapter, retain the client's capabilities from the request. `.await` drives the connection until it finishes. The helper macro supplies information the callback API needs; do not remove it because it looks redundant.

## 3. Build, then inspect real frames

```sh
cargo check --example acp_handshake
cargo build --locked --example acp_handshake
```

The first command resolves the new dependency and updates the lockfile; review that diff. The second requires the resolved lockfile to remain unchanged. `--example` selects the file under `examples/`; it does not replace the normal CLI binary.

**Rationale:** compilation tests the exact API/trait assumptions before involving Zed.

In Terminal A, run:

```sh
./target/debug/examples/acp_handshake
```

Paste this **single line**, then press Enter:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}}
```

Look for a response with `id: 1`, `result.protocolVersion: 1`, and agent capabilities. Your terminal may echo the line you typed; that is terminal echo, not the agent writing it. Press Ctrl-D on an empty line to close stdin.

Repeat with a fresh process and `protocolVersion: 2`. It must return **1**, the version it supports, not pretend to implement v2. A client unable to use the returned version should close the connection.

For an unambiguous stdout capture using a fresh process:

```sh
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}}' | ./target/debug/examples/acp_handshake > /tmp/acp-handshake-out.jsonl 2> /tmp/acp-handshake-err.log
python3 -m json.tool /tmp/acp-handshake-out.jsonl
```

`printf` sends one frame; the pipe connects it to stdin; the redirects separate stdout and stderr. `json.tool` parses/pretty-prints the single response. These `/tmp` paths are disposable lab outputs and this command overwrites them.

**Rationale:** successful JSON parsing catches banners and stray `println!` output. It is not full ACP validation.

## 4. Verify the rest of the project

```sh
cargo check --locked
cargo test --locked
```

Explain why a passing handshake does **not** demonstrate session support, Zed integration, cancellation, or full v1 conformance. Record only what you actually observed in [progress.md](progress.md).

## 5. Automated review gate

```sh
python3 guides/acp/check_handshake.py --report /tmp/acp-lab01-review.json --command "$PWD/target/debug/examples/acp_handshake"
```

`--report` saves actual observations as JSON and overwrites that disposable path. Passing means exit 0 and `passed` equals `total`; read each assertion, not only the summary. Diagnostics on stderr are allowed and counted, not treated as protocol messages.

| Supplied test | What it proves |
|---|---|
| `initialize_v1` | One JSON-RPC reply, request ID preserved, v1 selected |
| `preserve_string_request_id` | Correlation is not restricted to numeric IDs |
| `unsupported_version_selects_v1` | A v2 offer is answered with the agent's supported v1 |
| `client_capabilities_do_not_become_agent_promises` | Client filesystem/terminal support does not advertise agent image/audio/HTTP support |
| `unknown_method_is_correlated_error` | SDK returns `-32601` with the unknown request's ID |
| `empty_stdin_exits_cleanly` | EOF terminates without a banner or unsolicited frame |

Every case also checks JSON-only newline-framed stdout and bounded process exit. This is intentionally a **Lab 01** verifier: it rejects optional features outside the lab profile. Keep it on the teaching example, not a future fully featured agent unchanged.

**Prove the verifier can catch a regression:** in your example only, add `println!("starting");` before the builder. Build, run the verifier, observe failure, remove that line, build, observe green. This deliberate defect teaches why logging configuration alone cannot protect stdout. Do not retain the defect.

**Tests you own next:** carry the initialization cases into the integrated binary's subprocess tests in Lab 02. Later add malformed parameters and lifecycle-order tests; do not mistake the six supplied probes for the whole protocol suite.

**Submit for review:** your example and manifest/lockfile diff, `/tmp/acp-lab01-review.json`, and the answers below. Say “Verify ACP milestone 01.” Hermes will re-read your current code, run the same bounded checks, inspect the lockfile and confirm the original CLI still builds. No paid provider calls are needed.

## Questions for review

1. Why return `ProtocolVersion::V1` instead of `request.protocol_version`?
2. Which code owns stdout serialization?
3. Why can the lab run without `OPENROUTER_API_KEY`?
4. Which request handlers are missing before Zed can start a conversation?
5. Why must a long prompt handler release the SDK dispatch loop?

Next: [Lab 02 — a session-based echo in Zed](02-echo.md). Do not enable real Write/Bash tools in the echo exercise.
