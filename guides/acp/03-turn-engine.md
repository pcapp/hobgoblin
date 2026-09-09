# Lab 03 — One turn engine, two adapters

**User-visible milestone:** Zed answers two prompts in one conversation using retained context; a new conversation starts empty. The existing `-p` CLI still works. Provider errors are visible failures, not empty successful turns.

**Status:** generated coursework, not an implemented adapter or evidence of learner mastery. Peter types every application and test change described below.

[Course](README.md) · [Progress](progress.md) · [Previous: echo](02-echo.md) · [Next: tools](04-tools.md)

## Prerequisites and scope

- Complete stages 00–02: learner-owned newline framing, envelope/method DTOs, router, serialized writer queue, session IDs, echo updates, early cancellation, and integrated `--acp` startup without provider credentials.
- Preserve the standalone handshake and the scripted echo tests as no-network diagnostics. This lesson replaces the integrated echo answer source, not its transport.
- Target stable ACP v1 without ACP agent/schema SDK dependencies. Serde/serde_json and Tokio are allowed; the protocol types, request correlation, and task ownership remain yours.
- Real model requests are optional later evidence and need explicit approval; every acceptance test below uses a scripted provider.
- Keep real tools disabled until [Lab 04](04-tools.md). A scripted tool result can exercise the loop safely now.

## Why this boundary matters

A session owns durable history; a turn borrows or temporarily owns that history while it works. An adapter translates domain events, not stdout. A provider translates model-specific requests and responses, not ACP messages.

`&mut SessionState` expresses one writer. `Arc` shares ownership but does not serialize mutations. Use a short-lived map lock to reserve a session; move its history into the turn task and restore it on every exit, or use a per-session worker. Do not hold a global mutex across an await.

Keep `serde_json::Value` for the existing provider history initially; replacing every DTO at once hides the ownership lesson. Use small enums for outcomes/errors. Existing `async-openai`, Tokio, Serde, and tracing suffice; add a provider trait only because the scripted provider is a real second implementation. A generic async trait method avoids needing `async-trait` or a trait object initially.

Read [learner-owned protocol/tasks](file:///Users/peter/knowledge-bundles/harness-engineering/references/acp-rust-protocol.md), [API outcomes](file:///Users/peter/knowledge-bundles/harness-engineering/protocols/acp-api-map.md), and canonical [prompt outcomes](https://agentclientprotocol.com/protocol/v1/prompt-turn#stop-reasons).

## Before changing anything: reproduce the diagnosis

Read these **current** targets; line numbers are a starting snapshot, so re-find symbols after each refactor:

| Current target | Observation to explain |
|---|---|
| `src/agent.rs:7–16`, `run`, `messages`, `MAX_LOOPS` | Each invocation allocates fresh history; budget is local. |
| `src/agent.rs:31–55`, `create_byot`, `ChatResponse` | Provider decoding and history mutation are mixed; malformed/no-choice paths return `Ok(())`. |
| `src/agent.rs:59–77`, `println!`, `execute_tool_call` | Presentation and tool execution live inside the loop. |
| `src/agent.rs:81–86` | Request-budget exhaustion is reported as ordinary success. |
| `src/wire.rs:4–29`, `ChatResponse`, `Choice`, `Message` | Provider DTOs are not ACP schema types; `Choice` does not retain a finish reason. |
| `src/main.rs:28–44`, provider construction, `agent::run` | CLI setup must remain separate from protocol startup. |

Run from `/Users/peter/repos/codecrafters-claude-code-rust`:

```sh
cargo check --locked
cargo test --locked -- --list
git diff -- src Cargo.toml Cargo.lock
```

Write down which tests actually exist. A successful run with zero matching tests is not evidence of history isolation.

## Incremental learner actions

The proposed adapter path below is `src/acp/mod.rs`. If stage 02 used `src/acp.rs`, keep that single module root and place its child modules under `src/acp/`; do not create both roots. New paths are proposals, not files already present.

### 1. Give history an owner before changing the loop

Extend the `src/session.rs` created in Lab 02 with `SessionState` owning history and an absolute `PathBuf` cwd. Put session ID, busy/active-turn identity, and cancellation handle in the adapter's session registry. Initialize a fresh state once per `session/new`, not per prompt.

In `src/agent.rs::run`, first accept caller-owned history without changing provider behavior. In `src/main.rs`, create one temporary session for CLI invocation. **Rationale:** one ownership change can be checked independently of ACP serialization.

### 2. Make completion and failure different types

In `src/agent.rs`, introduce `TurnOutcome`, `TurnError`, and `AgentEvent`; replace each old `Ok(())` deliberately. Use this **domain design sketch**, not a wire definition or a complete solution:

```rust
pub enum TurnOutcome { EndTurn, MaxTokens, MaxTurnRequests, Refusal, Cancelled }
pub enum TurnError { Provider(String), MalformedResponse(String), EmptyChoices }
pub enum AgentEvent { AssistantText(String) }
```

Keep `Result<TurnOutcome, TurnError>` separate from event delivery failure. Extend `src/wire.rs::Choice` to preserve the provider's finish reason; explicitly handle stop, length, tool calls, and unknown/missing reasons according to the provider contract. Do not invent a refusal when the provider supplies none.

**Rationale:** budget exhaustion maps to `max_turn_requests`; malformed data is an error; a text chunk is neither of those.

### 3. Add a provider seam and a deterministic fake

Create `src/provider.rs:1` for `OpenRouterProvider` and the narrow request/response operation extracted from `src/agent.rs:19–51`. Put `ScriptedProvider` behind `#[cfg(test)]` in `src/provider.rs`; store an ordered queue of replies and captured requests. Fail if a test consumes an unplanned reply.

Choose a generic provider parameter for `run_turn`; add an explicit `Send` future bound only if your task executor requires it. Do not solve compiler lifetime errors by cloning whole sessions or retaining global locks.

For the first tests, return literal assistant replies and inspect the captured history. Do not ask a real model whether it remembers a word. **Rationale:** exact request assertions distinguish state retention from a lucky model answer.

### 4. Extract a presentation-free turn function

In `src/agent.rs`, extract `run_turn` from `run`: append the new user input once, request the provider, validate before committing assistant messages, emit `AssistantText`, and return a typed outcome. Preserve assistant tool-call IDs and matching tool results through a fake executor seam for now.

Document this local history policy: keep accepted user input and validated completed messages on failure/cancellation; never append malformed assistant data. Complete or remove dangling tool-call groups before the next provider request. A cancelled turn must not leave an invalid provider transcript.

For a budget of two model requests, check the budget before a third request, not after sending it. **Rationale:** deterministic bounds must constrain actual external work.

### 5. Reconnect CLI, then ACP

Keep `src/agent.rs::run` temporarily as a CLI wrapper, or move presentation to proposed `src/cli.rs:1`. Only that adapter prints human-readable answers. In `src/main.rs::main`, preserve the stage 02 mode split and lazy provider credentials; keep tracing on stderr and redact `payload` logging from `src/agent.rs:25–35`.

In the stage 02 prompt route in `src/acp/mod.rs`, validate and reserve the session without awaiting provider work, move its original request ID, final-response obligation, and owned turn state into an independently running Tokio task, then resume routing input. Track task handles under a connection supervisor; failed admission or a task failure must release the reservation and settle the obligation once, not strand a busy session. The reader/router must not await the turn's completion.

Await provider work and reverse-request oneshots only in turn tasks. The router delivers replies through Lab 02's bounded pending outbound request map while continuing to handle cancellation and other sessions. One finalizer owns each prompt's final-response obligation. Convert `AssistantText` into your own DTO for a `session/update` notification with `sessionUpdate: "agent_message_chunk"`; serialize through the single bounded writer queue. The current whole-response provider can legitimately emit one chunk, without claiming token streaming.

Write a table-driven mapper against canonical [v1 stop reasons](https://agentclientprotocol.com/protocol/v1/prompt-turn#stop-reasons): `EndTurn` → `end_turn`, `MaxTokens` → `max_tokens`, `MaxTurnRequests` → `max_turn_requests`, `Refusal` → `refusal`, and `Cancelled` → `cancelled`. A tiny **wire result fixture**, not a full response or captured traffic:

```json
{"stopReason":"max_turn_requests"}
```

Wrap that result in your response envelope using the unchanged inbound request ID. Construct your own Serde DTOs, with explicit wire names; map `TurnError` to a redacted JSON-RPC error instead of any successful stop reason. Map errors separately for CLI stderr/nonzero exit.

**Rationale:** a router awaiting slow work cannot process replies or cancellation. Spawning is not completion: never send early `end_turn`. Stop/join event producers, enqueue final updates then the response through one finalizer, restore session ownership, and clear busy state before admitting the next turn. FIFO stdout serialization alone cannot prevent a stray producer from enqueueing after completion; the finalizer must close that boundary too.

## Deterministic tests to type

Place domain tests in `src/agent.rs` under `#[cfg(test)]`; adapter tests in `src/acp/mod.rs`. Create shared in-memory peer/event recorder helpers in `src/acp/test_support.rs`, imported only for tests. No library crate split is needed to test private modules.

Use the function names below literally. For each test record **Arrange / Act / Assert**:

- **`acp_stage03_history_survives_two_turns`** — Arrange an empty session and two scripted replies. Act with “Remember cedar”, then “Repeat it”. Assert the second captured provider request contains the first user message, first assistant reply, and second user message exactly once and in order.
- **`acp_stage03_sessions_do_not_share_history`** — Arrange sessions A/B with distinct cwd and fakes. Act with a cedar prompt in A, then an unrelated prompt in B. Assert B's complete captured history has no A message or cwd.
- **`acp_stage03_provider_failures_are_errors`** — Arrange separate cases for transport failure, malformed JSON, and empty choices. Act once each. Assert one error response, no successful stop reason, no fabricated assistant text, and a reusable non-busy session.
- **`acp_stage03_budget_is_not_success`** — Arrange a budget of two and scripted tool-call rounds with fake results. Act until exhaustion. Assert exactly two provider requests and `MaxTurnRequests`, with no third request.
- **`acp_stage03_updates_precede_one_response`** — Arrange a gated provider and event recorder. Act by starting a prompt, observe provider-started, then release the reply. Assert no early response, text update before exactly one final response, and no later update for that turn.
- **`acp_stage03_busy_session_does_not_block_other_session`** — Arrange A waiting at a provider gate. Act with another prompt in A and one in B. Assert the second A request receives the documented busy error without history mutation; B completes while A remains gated.
- **`acp_stage03_cli_and_acp_present_same_answer`** — Arrange identical scripted replies and capture each adapter's sink. Act through both adapters. Assert CLI text equals ACP text content, but ACP stdout contains only protocol frames and CLI errors remain nonzero outcomes.

Use channels/barriers, not sleeps; each wait needs a bounded timeout so a deadlock fails rather than hangs. Have the peer read until the final response and explicitly drain/check late events.

```sh
cargo test --locked acp_stage03_ -- --list
cargo test --locked acp_stage03_ -- --nocapture
cargo check --locked
cargo test --locked
```

These commands are **future checks after you type the tests**, not claims they pass today. Verify every named test appears in `--list` and executes; inspect failures before proceeding.

## Acceptance gate

- [ ] All named tests pass without credentials, network, real tool effects, or timing guesses.
- [ ] Same-session continuity and cross-session isolation are shown by exact captured requests.
- [ ] Provider failure, request budget, cancellation, and normal completion have distinct mappings.
- [ ] CLI behavior survives; ACP stdout remains JSON-only; no early or duplicate final response.
- [ ] Unknown sessions and a concurrent same-session prompt leave history unchanged.
- [ ] Optionally, after approving model cost, demonstrate two Zed turns and a fresh thread; record this separately from deterministic evidence.

## Teach-back and evidence

Explain: who owns history during an await, who owns the original request ID and final-response obligation, why a spawned task must not acknowledge completion early, and how malformed provider data differs from budget exhaustion. Show the exact assertion that would catch accidental history reset, and the task that keeps routing reverse replies while a turn waits.

Record your explanation, commands, actual discovered test names/results, and remaining gaps in [progress](progress.md) during review. Generated lessons alone do not advance a learning state.

**Snippet status:** enums are domain sketches and the JSON result follows the canonical protocol. Historical SDK-expression compilation is superseded, not evidence for this path. No revised application/test implementation was compiled or run during this documentation revision; the adapter and Zed integration remain learner work.
