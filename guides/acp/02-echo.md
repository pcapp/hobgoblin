# Lab 02 — Start in ACP mode and echo into Zed

**Milestone:** Zed launches your existing binary with `--acp`, creates distinct conversations, and displays an echo. No model credentials, disk writes or shell tools. Your original `-p` CLI still works.

**Prerequisite:** [Lab 01](01-handshake.md) reviewed. This lab has two checkpoints; do not do the startup refactor and session state machine in one large patch. **Profile:** an incomplete teaching agent, not full ACP conformance; use empty MCP server lists until Lab 06.

## What to learn first

- An ACP **update** supplies display content; the final **prompt response** says why work stopped. Returning `end_turn` alone displays no answer.
- Session identity survives individual request IDs. Own a `PathBuf` for cwd and store state by `SessionId`; never call process-wide `set_current_dir` for a session.
- Initialization capabilities are connection state. Keep them separate from per-session history and per-turn cancellation.
- `move` gives an async task ownership of captured state. Shared ownership (`Arc`) is not permission for concurrent mutation. Keep lock scopes short and do not await provider/client work while holding them.
- Ordered callbacks run inside the SDK dispatch loop. Use connection-managed spawning for slow work and retain the responder until work finishes. Learn this now with slow echo, not after introducing tools.

Read: wiki [initialization](file:///Users/peter/knowledge-bundles/harness-engineering/concepts/acp-initialization.md), [API map](file:///Users/peter/knowledge-bundles/harness-engineering/protocols/acp-api-map.md), [SDK ordering](https://docs.rs/agent-client-protocol/2.1.0/agent_client_protocol/concepts/ordering/), and [building an agent](https://docs.rs/agent-client-protocol-cookbook/2.1.0/agent_client_protocol_cookbook/building_an_agent/).

## Files and crate choices

**You create:** `src/acp.rs`, `src/session.rs`, `tests/acp_sessions.rs`, `tests/cli_modes.rs`. These are proposed paths, not existing implementations. Keep private state unit tests beside their definitions; subprocess tests can exercise a binary without adding `src/lib.rs`.

**You modify:** `src/main.rs` (`Args`, startup before provider configuration), `Cargo.toml` only as needed. Do not refactor the model loop yet.

Use the existing `clap` derive support. Use standard-library `HashMap`, `PathBuf`, and a checked monotonic ID counter scoped to the connection; globally persistent IDs and a UUID crate are unnecessary while `loadSession` is false. A later persistence design may change that decision. If choosing `tokio_util::sync::CancellationToken`, add `tokio-util` as a direct dependency with `rt`; Tokio timers/select require the corresponding `time`/`macros` features. Confirm resolved versions in `cargo tree --locked` before copying APIs.

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

Move the handshake builder into a new `pub async fn run() -> agent_client_protocol::Result<()>` in `src/acp.rs`; register `mod acp` in `main.rs`. After `Args::parse`, branch: ACP invokes `acp::run` and returns; CLI continues through the existing dotenv/provider setup and passes its present prompt to `agent::run`. Preserve stderr tracing. Handle the CLI prompt explicitly rather than allowing an unrelated panic.

**Why:** process wiring becomes testable independently of provider configuration. The CLI path retains its existing behavior; the ACP path stays no-model until Lab 03.

### 4. Prove both launch modes

Create `tests/cli_modes.rs`. Use `env!("CARGO_BIN_EXE_codecrafters-claude-code")` to locate Cargo's integration-test binary. Start it with `--acp` from a temporary cwd, remove common model-key variables, send initialization, close stdin and assert a JSON reply/clean exit. Test `--help` and invalid arguments too. The test runner must concurrently drain stdout/stderr and kill/reap timed-out children; a pipe left undrained can hang tests.

Run `cargo test --locked --test cli_modes -- --nocapture` after creating that target. `--test` chooses the integration test; `--nocapture` shows its diagnostics. Before the routing fix, the no-key ACP test must fail; after, it must pass.

Checkpoint A review can also run the Lab 01 script against the built integrated binary with `--command "$PWD/target/debug/codecrafters-claude-code" --acp`. Its narrow capability expectations still apply at this point.

## Checkpoint B — Add sessions, then a turn

### 5. Create session state before prompt behavior

In `src/session.rs`, define a small store with a fresh-ID operation, lookup, and an explicit idle/running state. Store session cwd. For this lab choose a documented finite limit (for example 64 sessions per connection), reject new allocations at the limit, and use checked counter increments so IDs cannot wrap/reuse. Process exit drops the store; do not promise persistence.

Write pure tests first: two creates produce different IDs, lookup of an unknown ID fails, each session retains its own cwd, and a capacity failure preserves existing sessions. These tests require no SDK process or model.

In `src/acp.rs`, capture initialization state and client capabilities. Register `NewSessionRequest`; require initialized state, validate absolute cwd, and reject nonempty MCP lists explicitly as **not implemented in this lab**, rather than silently discarding them. Return `NewSessionResponse::new(id)` after storing the session. This restriction must be removed for final baseline support in Lab 06.

### 6. Build typed updates, not hand-written JSON

A small API example, independent of the store:

```rust
use agent_client_protocol::schema::v1::{
    ContentChunk, NewSessionResponse, PromptResponse,
    SessionId, SessionNotification, SessionUpdate, StopReason,
};

fn echo_frames(id: SessionId, text: String)
    -> (NewSessionResponse, SessionNotification, PromptResponse)
{
    let created = NewSessionResponse::new(id.clone());
    let update = SessionNotification::new(
        id,
        SessionUpdate::AgentMessageChunk(ContentChunk::new(text.into())),
    );
    (created, update, PromptResponse::new(StopReason::EndTurn))
}
```

This compiles as a payload-construction example, **not a server**. The new-session response belongs to `session/new`; do not send it during a prompt. In a `PromptRequest` callback, send the update with `connection.send_notification(update)?`, then finish that request with `responder.respond(done)` once the echo work finishes.

Match prompt content explicitly: concatenate text blocks in order; represent a resource link using its provided name/URI for echo without fetching it. Do not silently discard unsupported image/audio blocks. The echo representation is not finished resource-context resolution (Lab 06).

### 7. Preserve responsiveness with slow echo

Register `CancelNotification` using `on_receive_notification` and its matching helper macro. A notification has no response ID: **do not invent a cancel response**.

In the ordered prompt callback: validate session/content; reject a second simultaneous turn for the same session (local policy); mark running; create a fresh per-turn cancellation handle; move owned work and the prompt responder into `connection.spawn(...)`; return promptly. Read the pinned ordering example for exact spawn bounds instead of substituting detached `tokio::spawn` blindly.

Inside the task: race a controllable delay with cancellation; emit text only while active; finish once with `EndTurn` or `Cancelled`; clear active-turn state. Use one owner for terminal response/cleanup. Inject the delay or a release channel for tests; normal echo need not sleep. Do not block the ordered callback or hold the session-map lock across awaits.

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

**Snippet verification:** the exact argument declaration/test and payload-construction example were compiled in an isolated crate against Clap 4.6.5 and ACP SDK 2.1.0. Four parser cases and one payload smoke test passed. This does not establish an implemented session server or Zed interoperability.

## Review gate

Submit your diff, named test results, and Zed evidence. Explain why a completed prompt is not a completed session, why cancel has no response, and who owns the final prompt responder. Hermes re-runs deterministic tests; Zed behavior is verified separately. A passing echo is not approval to enable real writes or commands.

[Milestones](milestones.md) · [Previous](01-handshake.md) · [Next: turn engine](03-turn-engine.md)
