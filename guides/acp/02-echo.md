# Lab 02 — Start in ACP mode and echo into Zed

**Milestone:** Zed launches your existing binary with `--acp`, creates distinct conversations, and displays an echo. No model credentials, disk writes or shell tools. Your original `-p` CLI still works.

**Prerequisite:** [Lab 01](01-handshake.md) reviewed. This lab has two checkpoints; do not do the startup refactor and session state machine in one large patch. **Profile:** an incomplete teaching agent, not full ACP conformance; use empty MCP server lists until Lab 06.

## What to learn first

- An ACP **update** supplies display content; the final **prompt response** says why work stopped. Returning `end_turn` alone displays no answer.
- Session identity survives individual request IDs. Own a `PathBuf` for cwd and store state by a learner-defined session ID; never call process-wide `set_current_dir` for a session.
- Initialization capabilities are connection state. Keep them separate from per-session history and per-turn cancellation.
- `move` gives an async task ownership of captured state. Shared ownership (`Arc`) is not permission for concurrent mutation. Keep lock scopes short and do not await provider/client work while holding them.
- Your reader/router must remain responsive while independently running turn tasks await work. Move the original request ID and its one-response obligation into the turn task. Learn this now with slow echo, not after introducing tools.

Read: wiki [initialization](file:///Users/peter/knowledge-bundles/harness-engineering/concepts/acp-initialization.md), [API map](file:///Users/peter/knowledge-bundles/harness-engineering/protocols/acp-api-map.md), [learner-owned Rust protocol](file:///Users/peter/knowledge-bundles/harness-engineering/references/acp-rust-protocol.md), canonical [stdio transport](https://agentclientprotocol.com/protocol/v1/transports), and [JSON-RPC envelopes](https://www.jsonrpc.org/specification).

## Files and crate choices

**You create:** `src/acp.rs`, `src/session.rs`, `tests/acp_sessions.rs`, `tests/cli_modes.rs`. These are proposed paths, not existing implementations. Keep private state unit tests beside their definitions; subprocess tests can exercise a binary without adding `src/lib.rs`.

**You modify:** `src/main.rs` (`Args`, startup before provider configuration), `Cargo.toml` only as needed. Do not refactor the model loop yet.

Use the existing `clap` derive support, Serde/serde_json, and Tokio. **Do not add ACP agent or schema SDK dependencies:** you define the envelopes, method DTOs, router, and lifecycle. Use standard-library `HashMap`, `PathBuf`, and a checked monotonic session-ID counter scoped to the connection; globally persistent IDs and a UUID crate are unnecessary while `loadSession` is false. Request IDs are a separate namespace, not session IDs. A later persistence design may change that decision.

Lab 01 deliberately uses synchronous `std::io`; this lab migrates the integrated transport to Tokio so reading continues during turns. In your manifest, explicitly enable Tokio `io-util`, `io-std`, and `sync` in addition to the existing runtime/macros features; add `time` for deadlines. Use [Tokio I/O](https://docs.rs/tokio/latest/tokio/io/index.html), bounded [mpsc](https://docs.rs/tokio/latest/tokio/sync/mpsc/index.html), and [oneshot](https://docs.rs/tokio/latest/tokio/sync/oneshot/index.html) documentation, checking resolved versions with `cargo tree --locked`. If choosing `tokio_util::sync::CancellationToken`, add `tokio-util` directly with `rt`; Tokio `watch` is an alternative. These are learner changes, not dependencies installed by the guide.

## Checkpoint A — Separate startup from credentials

### 1. Inspect before changing

Read `src/main.rs:11–14` (`prompt: String`) and `:26–44` (unconditional provider setup). Predict what today's program does for `--acp`. Use `cargo run --locked -- --help` to inspect arguments without making a model request. The `--` separates Cargo arguments from program arguments.

**Why:** handshake must not fall through to `agent::run` or demand provider credentials. Do not run `run.sh`: it configures a proxy and launches a live model/tool request.

### 2. Write parser tests, then change `Args`

Start with unit tests next to `Args` in `src/main.rs`, using `Args::try_parse_from`. Specify: `-p hello` accepted, `--acp` accepted, neither rejected, both rejected. Run the tests before changing the parser and observe failures.

This is the complete replacement argument declaration to type when those tests are red (keep `use clap::Parser`):

```rust
#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    #[arg(long, conflicts_with = "prompt")]
    acp: bool,
    #[arg(short = 'p', long, required_unless_present = "acp")]
    prompt: Option<String>,
}
```

Example assertion inside a `#[cfg(test)]` test module with `use super::*`:

```rust
#[test]
fn acp_needs_no_prompt() {
    let args = Args::try_parse_from(["agent", "--acp"]).unwrap();
    assert!(args.acp);
    assert!(args.prompt.is_none());
}
```

**Why:** `Option<String>` models the absence of a CLI prompt; Clap enforces the relationship. Avoid manufacturing an empty prompt for ACP mode.

### 3. Route before provider setup

Move the learner-owned handshake routing into a new async `run` entry point in `src/acp.rs`, returning your own error type (or the application's existing boxed-error convention); register `mod acp` in `main.rs`. Keep the standalone Lab 01 diagnostic synchronous. After `Args::parse`, branch: ACP invokes `acp::run` and returns; CLI continues through the existing dotenv/provider setup and passes its present prompt to `agent::run`. Preserve stderr tracing. Handle the CLI prompt explicitly rather than allowing an unrelated panic.

Migrate integrated stdin/stdout handling before adding sessions. Own a bounded UTF-8 newline decoder: buffer partial reads and extract every complete frame from coalesced reads. Preserve Lab 01's local limit of **64 KiB including the terminating newline**, checked before unbounded allocation. EOF with an empty buffer exits cleanly; unterminated EOF or oversized input reports to stderr and closes nonzero. A complete malformed JSON frame returns `-32700` with `id: null`, then reading continues. This individual-message profile does not accept batch arrays. A newline inside a JSON string must be escaped, not a physical line break; this is not `Content-Length` framing. Do not run blocking `std::io` reads on the async dispatch task, or allocate an unlimited line before checking its size.

Make exactly one writer task own stdout. Producers enqueue complete compact JSON frames on a bounded queue; the writer adds one newline, uses `write_all`, and flushes. No turn prints directly. Choose a bounded enqueue/write deadline and supervisor shutdown policy if the peer stops reading: do not block the reader/router forever on a full output queue. Writer failure must signal connection teardown and settle waiting tasks, not silently discard replies. A successful enqueue is not proof the peer received the frame.

Check [Tokio stdin's shutdown caveat](https://docs.rs/tokio/latest/tokio/io/fn.stdin.html): its underlying blocking read cannot be cancelled, so aborting the async reader is not by itself proof the process can exit while stdin stays open. Exercise writer failure with stdin still open; design a bounded top-level shutdown policy after owned-resource cleanup, and retain the subprocess harness's kill/reap watchdog. Do not claim cancellation of a runtime task cancels the underlying OS read.

**Why:** process wiring becomes testable independently of provider configuration. The CLI path retains its existing behavior; the ACP path stays no-model until Lab 03.

### 4. Prove both launch modes

Create `tests/cli_modes.rs`. Use `env!("CARGO_BIN_EXE_codecrafters-claude-code")` to locate Cargo's integration-test binary. Start it with `--acp` from a temporary cwd, remove common model-key variables, send initialization, close stdin and assert a JSON reply/clean exit. Test `--help` and invalid arguments too. The test runner must concurrently drain stdout/stderr and kill/reap timed-out children; a pipe left undrained can hang tests.

Run `cargo test --locked --test cli_modes -- --nocapture` after creating that target. `--test` chooses the integration test; `--nocapture` shows its diagnostics. Before the routing fix, the no-key ACP test must fail; after, it must pass.

Checkpoint A review can also run the Lab 01 script against the built integrated binary with `--command "$PWD/target/debug/codecrafters-claude-code" --acp`. Its narrow capability expectations still apply at this point. The supplied six probes do not prove flushing before EOF: write your own open-stdin test that receives initialize's response without closing input, with a timeout and child cleanup.

## Checkpoint B — Add sessions, then a turn

### 5. Create session state before prompt behavior

In `src/session.rs`, define a small store with a fresh-ID operation, lookup, and an explicit idle/running state. Store session cwd. For this lab choose a documented finite limit (for example 64 sessions per connection), reject new allocations at the limit, and use checked counter increments so IDs cannot wrap/reuse. Process exit drops the store; do not promise persistence.

Write pure tests first: two creates produce different IDs, lookup of an unknown ID fails, each session retains its own cwd, and a capacity failure preserves existing sessions. These tests require no protocol process or model.

In `src/acp.rs`, capture initialization state and client capabilities. Route `session/new` to your own parameter DTO; require initialized state, validate absolute cwd, and reject nonempty MCP lists explicitly as **not implemented in this lab**, rather than silently discarding them. Return a result object containing `sessionId` after storing the session, correlated to that request's unchanged ID. This restriction must be removed for final baseline support in Lab 06. MCP is separate from ACP; its library decision is deferred, not imposed here.

### 6. Define your DTOs and route all three envelope kinds

Define small Serde structs/enums for the method subset you support, with explicit wire names such as `sessionId`, `sessionUpdate`, and `stopReason`. Use `serde_json::Value` for initial envelope inspection and extension metadata where useful; neither Serde nor a Rust type replaces envelope validation. Preserve permitted unknown fields without interpreting them as capabilities or authority.

Your router distinguishes requests (`method` and an ID), notifications (`method`, no ID), and replies (ID with exactly one of `result`/`error`, no method). Check field **presence**, not truthiness: `result: null` is still a result. Missing ID is not the same as an explicit null ID. Retain the Lab 01 ID policy and preserve supported string/numeric IDs exactly, including their type. Never coerce `"7"` to `7`.

For inbound requests, retain one response obligation until validation fails or the owning task completes. For outbound requests, allocate your own checked non-reused ID and keep a **separate, bounded pending map** from ID to method/session/turn context and a oneshot sender. Register before enqueueing the request so an immediate reply cannot be lost. The reader/router removes the matching entry and delivers the result/error to its waiting task; it never waits for that task. Exercise this path with a fake peer now; real reverse requests arrive in Lab 04. Define failure/timeout/disconnect removal and ignore/log unknown or duplicate replies without sending a reply to a reply. Inbound and outbound IDs may have identical values: direction and envelope kind disambiguate them.

These **illustrative wire fixtures** follow canonical [session setup](https://agentclientprotocol.com/protocol/v1/session-setup) and [prompt turn](https://agentclientprotocol.com/protocol/v1/prompt-turn), not a library's serialization. They are separate newline-terminated frames, not captured traffic. IDs 20 and 30 refer to different client requests:

```json
{"jsonrpc":"2.0","id":20,"result":{"sessionId":"s1"}}
```

```json
{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hello"}}}}
```

```json
{"jsonrpc":"2.0","id":30,"result":{"stopReason":"end_turn"}}
```

The session-created result belongs only to `session/new`. In a prompt task, enqueue text updates, then its one final response after work finishes. Write a serialization test comparing your DTO's JSON value against these fixtures; do not compare object-key order or manually concatenate unescaped text.

Match prompt content explicitly: concatenate text blocks in order; represent a resource link using its provided name/URI for echo without fetching it. Do not silently discard unsupported image/audio blocks. The echo representation is not finished resource-context resolution (Lab 06).

### 7. Preserve responsiveness with slow echo

Route `session/cancel` as a notification and validate its session parameters. A notification has no response ID: **do not invent a cancel response**.

In the prompt route: validate session/content; reject a second simultaneous turn for the same session (local policy); mark running; create a fresh per-turn cancellation handle; move owned work, original request ID, and writer-queue access into an independently running Tokio task; return promptly to reading/routing. Keep its `JoinHandle` or a supervised task set so failures/EOF cannot orphan work. With `tokio::spawn`, captured state/futures must satisfy its `Send + 'static` bounds; move owned values, not borrows from the input buffer. Give a supervisor a defined cleanup path for task failure without creating a second normal-response owner.

Inside the task: race a controllable delay with cancellation; emit text only while active; finish once with wire `end_turn` or `cancelled`; clear active-turn state. Use one owner for final response/cleanup, and close all event producers before enqueueing that response on the serialized writer queue. Inject the delay or a release channel for tests; normal echo need not sleep. Do not block the reader/router or hold the session-map lock across awaits.

**Why:** the dispatch loop must receive `session/cancel` while the prompt response is still pending. Lab 05 hardens the same structure for model and tool work.

## Tests you write for Checkpoint B

Create `tests/acp_sessions.rs`. Use a scripted client that leaves stdin open, sends each request after the previous dependency is satisfied, and reads until the matching response ID; notifications may arrive between responses. Close stdin only after turn assertions. An EOF immediately after `prompt` can tear down spawned work and is not a valid conversational test.

| Test name | Arrange → act → required assertion |
|---|---|
| `new_requires_initialize` | Fresh process → session/new → correlated error, no session allocated |
| `distinct_session_ids` | Initialize → create twice → different IDs, correct request correlation |
| `unknown_session_has_no_updates` | Initialize → prompt unknown ID → error, no update |
| `echo_update_precedes_completion` | Create A → prompt `hello` → A's text update before exactly one `end_turn` |
| `resource_link_echo_is_not_dropped` | Create A → text + resource-link prompt → both represented, no network fetch |
| `same_session_busy_is_rejected` | Start held-open turn → second prompt for A → second fails without disturbing first |
| `cancel_slow_echo` | Wait for fake work to start → send cancel → one `cancelled`, no late text, next turn works |
| `sessions_do_not_block_each_other` | Hold A's work → prompt B → B completes while A remains pending |
| `session_limit_is_bounded` | Fill configured capacity → create again → explicit error, previous sessions still usable |
| `async_framing_preserves_lab01_contract` | Split/coalesce frames, include escaped newlines, malformed JSON and oversized/incomplete input → same bounded framing/error policy, no partial-frame dispatch |
| `writer_serializes_concurrent_turns` | Release A/B producers together → every stdout line parses, each turn's update precedes its own one response, no byte interleaving |
| `reverse_replies_correlate_without_blocking` | Fake peer receives two outbound requests and replies in reverse order while a prompt is pending → correct oneshot wakes, string/numeric and inbound/outbound IDs remain distinct |
| `transport_limits_and_failure_settle_waiters` | Fill pending/output limits and separately fail writer or close input → bounded rejection/teardown, no hung waiters or orphan turn tasks |

Use channels/barriers to prove work started instead of fragile sleeps. Timeouts are failure bounds, not synchronization. A practical local cancellation deadline can be one second; that is a test budget, not an ACP specification requirement.

```sh
cargo test --locked --test acp_sessions -- --nocapture
cargo test --locked
cargo build --locked
```

These Rust test targets are **assignments**, not supplied passing tests. Ask for one exact test scaffold at a time if unfamiliar with subprocess I/O; do not invent a large harness before the first session test.

## Finally, connect Zed

Only after the tests pass, use Zed Agent Settings → External Agents → Add Agent → Add Custom Agent. Configure the absolute built executable and `--acp`; do not use `run.sh`. Preserve existing settings and omit credentials. See [Zed's documentation](https://zed.dev/docs/ai/external-agents) and the [course setup notes](README.md#zed-setup-only-after-sessionprompt-works).

Use `dev: open acp logs`. Start two threads, echo distinct strings, and inspect initialization → new session → prompt → update → response. Record actual Zed version and a redacted transcript. A screenshot proves UI rendering, not session isolation on its own.

**Validation scope:** these are learner exercises and canonical wire illustrations, not a supplied server. Earlier SDK-based snippet checks are superseded and do not validate this implementation path. No revised Rust transport or Zed integration was compiled/run during this documentation revision; the named tests must be implemented and exercised by the learner.

## Review gate

Submit your diff, named test results, and Zed evidence. Explain why a completed prompt is not a completed session, why cancel has no response, who owns the original prompt ID/final-response obligation, and how a reverse reply reaches its oneshot without blocking the router. Hermes re-runs deterministic tests; Zed behavior is verified separately. A passing echo is not approval to enable real writes or commands.

[Milestones](milestones.md) · [Previous](01-handshake.md) · [Next: turn engine](03-turn-engine.md)
