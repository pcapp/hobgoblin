# Lab 01 — Build an ACP handshake

## Goal

Build a Rust program that receives an initialization request over standard input and returns a matching response over standard output.

This is the first connection between an editor and an agent. The editor announces its protocol version and capabilities; the agent replies with its own. Initialization does not create a conversation or call a model.

**By the end, your program will:**

- Read newline-delimited JSON messages.
- Validate and route initialization requests.
- Reply with its supported ACP version and the original request ID.
- Return errors for invalid requests and stay silent for notifications.
- Keep diagnostics separate from protocol output and exit cleanly when input closes.

## Before you begin

Complete [Lab 00 — One agent, two frontends](00-architecture.md) and its source map. This lab isolates the protocol handshake in a separate executable; the shared turn engine is connected later.

You need Rust/Cargo and `uv` installed, and the existing Rust project building successfully. Familiarity with `Result`, `Option`, JSON objects, and basic Rust tests will help.

**File to create:** `examples/acp_handshake.rs`. Put this lab's functions and unit tests in that file. A Cargo example is a separate executable, so you can work without changing the existing application in `src/`.

**Libraries:** use the project's existing `serde` and `serde_json` dependencies, plus `std::io`. No dependency changes are needed. Synchronous I/O is sufficient because initialization finishes immediately; Lab 02 introduces Tokio when operations need to run concurrently.

Open a terminal and run:

```sh
cd /Users/peter/repos/codecrafters-claude-code-rust
pwd
cargo check --locked
```

`cd` selects the Rust repository. `pwd` should print that same path. Run the remaining Cargo commands from this directory. `cargo check` type-checks the project; `--locked` prevents changes to dependency resolution.

**Checkpoint:** the existing project builds. If it does not, resolve that baseline failure before adding the example.

## 1. Understand the exchange

ACP uses JSON-RPC envelopes to identify messages. Over standard input/output, each message is UTF-8 JSON followed by a newline. This complete line is called a **frame**.

An initialization request looks like this:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}}
```

Your program should produce this response, followed by a newline:

```json
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{}}}
```

| Field | Meaning |
|---|---|
| `jsonrpc` | The envelope format: JSON-RPC 2.0 |
| `id` | Identifies a request; its response must preserve the value and JSON type |
| `method` | Selects the operation to perform |
| `params` | Supplies that operation's inputs |
| `result` | Contains a successful operation's output |
| `protocolVersion` | The ACP version offered or selected, distinct from the JSON-RPC version |
| `clientCapabilities` | Features the editor supports |
| `agentCapabilities` | Features your agent supports |

Your program supports ACP **v1**. When the client offers v1, return v1. When it offers an unsupported version such as v2, return your latest supported version: v1. The client decides whether it can continue.

Return an empty `agentCapabilities` object for this lab. Do not copy the client's capabilities: the editor's ability to read a file does not mean your agent implements that feature.

**Checkpoint:** explain why a request with `id: "init-A"` must receive `id: "init-A"`, not `id: 1` or a newly generated ID.

## 2. Implement initialization without I/O

Create `examples/acp_handshake.rs`. Begin with a message-handling function and unit tests; leave stdin/stdout wiring for Step 4.

Use these function responsibilities to organize your work:

| Function | Input | Output |
|---|---|---|
| `handle_message` | A parsed `serde_json::Value` | `Some(response)` or `None` when no reply is required |
| `read_frame` | A buffered byte reader | One complete frame, clean EOF, or a framing error |
| `write_frame` | A JSON value and byte writer | A compact JSON line, or an I/O error |
| `run` | A reader and writer | Repeated read → parse → handle → write until EOF or an I/O failure |
| `main` | Process stdin/stdout | Runs the transport and reports failures to stderr |

You choose the Rust signatures. Accept `BufRead` and `Write` implementations at the transport boundary so tests can use `Cursor` and `Vec<u8>` instead of launching a process.

First, implement only the valid initialization path in `handle_message`:

1. Read the request ID without converting its type.
2. Recognize the `initialize` method.
3. Validate that `params.protocolVersion` is an integer and `params.clientCapabilities` is an object.
4. Construct a response with `jsonrpc: "2.0"`, the original ID, and the result shown in Step 1.

Use `serde_json::Value` to inspect the envelope. You may define Serde-derived structs for method parameters. Build JSON through serialization rather than concatenating strings.

Write these tests alongside the function in a `#[cfg(test)]` module:

| Test | Input → expected result |
|---|---|
| `initialize_v1` | Version 1, numeric ID → version 1, same ID |
| `initialize_string_id` | ID `"init-A"` → unchanged string ID |
| `initialize_zero_id` | ID `0` → unchanged numeric ID; zero is not missing |
| `initialize_unsupported_version` | Version 2 → version 1 |
| `initialize_separate_capabilities` | Client filesystem/terminal capabilities → empty agent capabilities |

After adding a `main` entry point so Cargo can compile the example, run:

```sh
cargo test --locked --example acp_handshake initialize
```

`--example` selects your example's test target. The trailing `initialize` filters test names. Check that your tests were discovered and passed; zero matching tests is not a completed checkpoint.

## 3. Add message classification and errors

Before dispatching a method, validate the envelope: it must be an object with `jsonrpc: "2.0"` and a valid message shape.

| Shape | Action |
|---|---|
| String `method` and an `id` | Handle a request and produce one response |
| String `method`, no `id` | Handle a notification without producing a response |
| No `method`, `id`, and exactly one of `result` or `error` | Recognize a response; this program has no outgoing requests, so ignore it |

Check **field presence**, not truthiness. `result: null` is still a result, and `id: 0` is still an ID. For this lab, support integer and string request IDs. Reject other request-ID shapes as invalid requests rather than treating them as notifications. JSON-RPC also permits null request IDs; this exercise deliberately uses the narrower integer/string request profile. Error responses may use a null ID when no valid request ID is available.

Implement these error cases:

| Problem | Error code | Response ID |
|---|---|---|
| Complete frame contains malformed JSON | `-32700` — Parse error | `null` |
| Invalid request envelope | `-32600` — Invalid request | Valid request ID if established; otherwise `null` |
| Unknown request method | `-32601` — Method not found | Original request ID |
| Invalid initialization parameters | `-32602` — Invalid params | Original request ID |

For example, an unknown method in request 2 produces:

```json
{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"Method not found"}}
```

A response must contain **either** `result` **or** `error`, never both. Valid notifications receive no response, including when the method is unknown or its parameters are unusable. Unexpected peer responses must not trigger another response.

Add tests for unknown requests, unknown notifications, invalid parameters, invalid envelopes, and unexpected peer responses. Then run all tests in the example:

```sh
cargo test --locked --example acp_handshake
```

**Checkpoint:** a request with an unknown method returns a correlated error; the same method without an ID produces no reply.

## 4. Implement the transport

Connect the handler to the stream:

```text
stdin → read_frame → parse JSON → handle_message → write_frame → stdout
```

### Read one bounded frame

Implement `read_frame` using buffered byte input. A stream read may contain part of a frame or several frames; do not assume one read equals one message.

Use this lab's framing policy:

- A frame ends at a newline. Preserve unused bytes for the next call.
- Limit each frame to **64 KiB, including its terminating newline**. Enforce the limit while reading, before allocating an oversized buffer. This is a local resource limit, not an ACP requirement.
- EOF with no pending bytes is clean shutdown.
- EOF with an unterminated frame, oversized input, or invalid UTF-8 is a transport failure: report to stderr and exit nonzero.
- A complete UTF-8 frame containing malformed JSON receives the parse-error response from Step 3; continue reading subsequent frames.
- Process individual message objects, not JSON batch arrays.

`BufRead::fill_buf` and `consume` let you inspect available bytes and advance the reader. If using another reading API, verify that it enforces the bound before growing the buffer. Checking length only after an unbounded `read_line` does not bound allocation.

### Write one complete response

Implement `write_frame` to serialize compact JSON, append one newline, and flush the writer. Flushing matters: the client waits for a reply while keeping stdin open.

An escaped `\n` within a JSON string is data. A physical newline terminates the frame. Do not pretty-print protocol output or print a startup banner. Propagate write failures rather than continuing on a broken pipe.

### Wire up the process

Implement `run` as the read/parse/handle/write loop. Keep `main` small: obtain stdin/stdout, call `run`, and report fatal errors to stderr with a nonzero exit status. Do not call the existing application's model/provider startup.

Write transport tests with in-memory readers and writers:

| Test | Expected behavior |
|---|---|
| `empty_input_exits_cleanly` | No input produces no output |
| `two_frames_are_processed` | Two request lines produce two response lines |
| `split_frame_is_reassembled` | A reader yielding fragments still produces one complete response |
| `malformed_json_is_parse_error` | A malformed line produces `-32700`; the following valid request still works |
| `frame_limit_is_enforced` | Boundary-size input is handled; oversized input fails without unbounded allocation |
| `incomplete_eof_is_rejected` | JSON without its terminating newline fails |
| `invalid_utf8_is_rejected` | Invalid UTF-8 fails without dispatching a request |
| `output_is_one_json_line` | Each reply parses as JSON and ends with a newline |

**Checkpoint:** all example tests pass. Explain which tests exercise framing and which exercise request handling.

## 5. Build and inspect a live exchange

Build and start the example:

```sh
cargo build --locked --example acp_handshake
./target/debug/examples/acp_handshake
```

Paste the initialization request from Step 1 and press Enter. The reply must appear **before** you close stdin. Your terminal may also echo the line you typed; that echo is not your program's output.

Press Ctrl-D on an empty line to close stdin. Repeat with a fresh process and `protocolVersion: 2`; verify that your response selects version 1.

**Checkpoint:** the process replies while input remains open and exits cleanly after EOF. Add a bounded subprocess test named `reply_does_not_wait_for_eof` to preserve this behavior. Keep the child's stdin open until the reply arrives, drain stdout/stderr concurrently, and kill/reap the child if the test times out.

## 6. Run the supplied checker

The checker launches your executable as a subprocess and inspects its actual stdin/stdout exchange. This absolute-path command works from any directory:

```sh
uv run --script /Users/peter/repos/codecrafters-claude-code-rust/guides/acp/check_handshake.py \
  --report /tmp/acp-lab01-review.json \
  --command /Users/peter/repos/codecrafters-claude-code-rust/target/debug/examples/acp_handshake
```

- `uv run --script` runs the checker with its declared Python environment.
- `--report` saves observations to a disposable JSON file, overwriting it on each run.
- `--command` must be last; everything after it belongs to the child executable.

The checker covers initialization, string IDs, version negotiation, separate capabilities, unknown methods, and clean EOF. It uses fresh processes, a temporary working directory, and a five-second timeout per case. It removes common model-key variables; it is a test runner, not a security sandbox.

**Expected result:** exit status 0 with `passed` equal to `total`. A missing-executable error means you need to build the example or correct its path. The checker closes stdin after sending input, so retain your own tests for flushing before EOF, malformed messages, and frame limits.

Prove the checker detects invalid output: temporarily add `println!("starting");` before the transport loop, rebuild, and observe a failing check. Remove the line, rebuild, and confirm the check passes again.

Finally, run these from the Rust repository:

```sh
cargo check --locked
cargo test --locked
cargo test --locked --example acp_handshake
```

## Completion checklist

- [ ] The example builds without changing the existing CLI or dependencies.
- [ ] Unit tests cover initialization, message classification, errors, and bounded framing.
- [ ] A subprocess test proves the response arrives before EOF.
- [ ] All supplied checker probes pass.
- [ ] The temporary stdout banner is removed and checks pass again.
- [ ] The existing project still builds and its tests pass.

Bring your example diff, test output, and `/tmp/acp-lab01-review.json` to review. Be ready to explain:

1. How does your program find the end of a message?
2. Why must a response preserve the request ID's value and type?
3. Why are a notification and an unknown request handled differently?
4. Why does a v2 offer receive a v1 result from this program?
5. Why must stdout be flushed, and where do diagnostics go?

This lab establishes initialization only. [Lab 02](02-echo.md) adds asynchronous transport, sessions, and an echo response that Zed can display.

## Reference

- [ACP initialization](https://agentclientprotocol.com/protocol/v1/initialization)
- [ACP stdio transport](https://agentclientprotocol.com/protocol/v1/transports)
- [JSON-RPC envelopes and errors](https://www.jsonrpc.org/specification)
- [`BufRead`](https://doc.rust-lang.org/std/io/trait.BufRead.html) and [`Write`](https://doc.rust-lang.org/std/io/trait.Write.html)
- [`serde_json::from_slice`](https://docs.rs/serde_json/1.0.151/serde_json/fn.from_slice.html) and [`serde_json::to_writer`](https://docs.rs/serde_json/1.0.151/serde_json/fn.to_writer.html)
