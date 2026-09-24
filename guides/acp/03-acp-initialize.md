# Task 3 — Add ACP initialization to the application

## End goal

The existing application starts in ACP mode with `--acp`, reads newline-delimited JSON-RPC from stdin, and completes ACP v1 initialization over stdout.

This must work without a provider API key because initialization does not run the agent core or call a model.

There is no separate example executable. The final checker launches the real binary with `--acp`.

## Concepts

### Startup chooses a frontend

The process must select its frontend before constructing frontend-specific dependencies:

```text
parse arguments
├─ `-p` or no args → configure provider → run terminal frontend
└─ `--acp`         → run ACP transport
```

If provider configuration happens first, Zed cannot initialize the agent on a machine where model credentials are unavailable. The protocol handshake and model authentication are separate concerns.

### ACP stdio is a framed protocol

ACP stdio carries one compact JSON message per physical line. A newline ends a frame. An escaped `\n` inside a JSON string is data, not a frame boundary.

In ACP mode:

- stdin contains protocol frames from the client.
- stdout contains protocol frames from the agent.
- stderr contains diagnostics.

A greeting, terminal prompt, or raw assistant answer on stdout corrupts the protocol.

### JSON-RPC IDs correlate requests and responses

An initialization request includes an ID. Its response must preserve both the ID value and its JSON type. The protocol version inside ACP params is separate from the envelope's `"jsonrpc": "2.0"` field.

At this stage, the agent supports ACP protocol version 1 and advertises only capabilities it actually implements—which is none beyond initialization itself.

### Build the retained transport with Tokio

Initialization could be implemented synchronously, but later ACP prompts must remain responsive while model or tool work is pending. Build the integrated stdio boundary with Tokio now so it can evolve rather than be replaced.

This does not require designing all future concurrency. It does require using async stdin/stdout APIs and keeping protocol I/O separate from the agent core.

## Inspect before changing

Review:

- `src/main.rs`: argument parsing, provider setup, and mode routing.
- `Cargo.toml`: the currently enabled Tokio features.
- `src/wire.rs`: provider API types that must not become ACP types.
- [`check_handshake.py`](check_handshake.py): the externally observed contract.

Read the canonical references before choosing your wire structures:

- [ACP initialization](https://agentclientprotocol.com/protocol/v1/initialization)
- [ACP stdio transport](https://agentclientprotocol.com/protocol/v1/transports)
- [JSON-RPC 2.0](https://www.jsonrpc.org/specification)
- [Tokio I/O](https://docs.rs/tokio/latest/tokio/io/index.html)

Before implementing, decide:

1. Which module owns ACP framing and dispatch?
2. Which Serde types are useful, and where is `serde_json::Value` safer for initial envelope classification?
3. How will one component retain exclusive ownership of stdout?
4. How will clean EOF differ from malformed or incomplete input?
5. Which Tokio features must be enabled explicitly?

## Required behavior

### Mode routing

- Add `--acp` as a mode that conflicts with `-p`.
- No arguments still select interactive terminal mode.
- Select ACP mode before loading dotenv or requiring `OPENROUTER_API_KEY`.
- Terminal modes retain their existing provider setup.

### Framing

- Read newline-terminated UTF-8 JSON frames asynchronously.
- Process frames until clean EOF.
- Write each response as compact JSON followed by one newline.
- Flush responses so a client does not need to close stdin before receiving them.
- Keep all human-readable diagnostics on stderr.
- Use a finite frame-size limit and reject oversized or unterminated input without unbounded allocation. The previous course used 64 KiB including the newline; retaining that local policy is reasonable.

### Message handling

For an `initialize` request:

- Require JSON-RPC version `2.0`.
- Preserve a supported numeric or string request ID exactly.
- Validate the initialization params needed by ACP.
- Select ACP protocol version 1.
- Return an `agentCapabilities` object that does not claim unimplemented features.
- Do not create a conversation, configure a provider, or call the model.

Also distinguish these JSON-RPC shapes:

- A request has a method and an ID and receives one correlated response.
- A notification has a method but no ID and receives no response.
- A response contains an ID and exactly one of `result` or `error`; do not respond to a response.

The supplied checker requires an unknown request method to receive a correlated `-32601` Method Not Found error. Handle malformed JSON and invalid request shapes deliberately even though the checker does not cover every negative case.

## Implementation boundaries

- Define ACP wire types separately from the OpenRouter types in `src/wire.rs`.
- The ACP frontend must not invoke the agent core during initialization.
- Avoid string concatenation for JSON; serialize values or Serde types.
- Do not add an ACP SDK or external ACP schema crate.
- Do not implement `session/new`, prompts, tool calls, or cancellation yet.
- Design for one serialized stdout owner. Initialization may dispatch sequentially, but future tasks must not allow concurrent work to interleave output bytes.

The exact modules, enums, and function signatures are your design.

## Suggested implementation path

1. Change argument parsing and route `--acp` before provider setup.
2. Enable the Tokio features needed for async stdin/stdout and buffered I/O.
3. Add a bounded frame reader and compact frame writer.
4. Classify JSON-RPC envelopes before method-specific decoding.
5. Implement initialization and unknown-method responses.
6. Keep reading until EOF.
7. Build the real binary and run the external checker.

This is one task with one concrete endpoint; these steps are not separate miniature assignments.

## Machine-verifiable acceptance

Run the local checks:

```sh
cargo fmt --all -- --check
cargo check --locked
cargo test --locked
cargo build --locked
cargo run --locked -- --help
```

Then run the subprocess checker against the actual application:

```sh
uv run --script guides/acp/check_handshake.py \
  --report /tmp/acp-initialize-review.json \
  --command /absolute/path/to/target/debug/codecrafters-claude-code --acp
```

Replace the executable path with the absolute path on your machine. `--command` must be last because all remaining arguments are passed to the child process.

The checker launches the application without common model credentials and from a temporary working directory. It verifies initialization, request-ID preservation, version negotiation, conservative capabilities, unknown-method errors, clean EOF, JSON-only stdout, and a flushed response before EOF.

Task 3 is complete only when every command exits with status 0 and the checker report has `passed` equal to `total`. No manual protocol exchange, live model call, or written explanation counts toward completion.

Record the commands, exit statuses, and report path in [progress.md](progress.md). The next course tasks should be designed from the implementation you now have, beginning with ACP session creation and prompt delivery.
