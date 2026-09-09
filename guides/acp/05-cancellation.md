# Lab 05 — Cancel work, not just the spinner

**User-visible milestone:** cancelling during a model request, approval, or command stops the turn promptly. Zed receives one `cancelled` completion, no later turn updates, and can start another turn without stale work or leaked terminals.

**Status:** generated coursework, not an implemented cancellation system. Peter writes the Rust and tests.

[Course](README.md) · [Progress](progress.md) · [Previous: tools](04-tools.md) · [Next: compatibility](06-compatibility.md)

## Prerequisites

- Stage 02's slow echo is already cancellable; Lab 03 owns session state and the original prompt's final-response obligation in an independent turn task, not the reader/router.
- Lab 04 has gated permissions, typed tool results, client terminals, and explicit release on normal/error paths.
- Keep stable ACP v1 with learner-defined Serde DTOs, framing, router, and writer queue; no ACP agent/schema SDK dependencies. Tokio remains allowed.
- All core tests use fake providers/peers. A separately gated scratch-process test is needed to prove an actual child exits.

## Concepts and crate choices

Cancellation is a state transition and cleanup protocol. A notification has no response ID; the original prompt still has an owned ID and response obligation. Dropping a future only stops polling it: it does not establish that a remote request, spawned task, or operating-system child stopped.

Use a fresh cancellation signal for each active turn, not a reusable session-wide “true” flag. A turn sequence/generation prevents stale completions from clearing the next turn's state. One finalizer owns the prompt's request ID/one-response obligation and the update stream's closing boundary.

Choose either Tokio `watch` or `tokio-util::sync::CancellationToken` as the signal. `tokio-util` 0.7 with its `rt` feature is an optional direct dependency if you choose the token; do not import a transitive dependency implicitly. Tokio `sync`/`time` support channels and timeouts; retain Lab 02's `io-util`/`io-std` transport features, add `test-util` only for virtual-time tests, and `process` for a real-child fixture. These are learner manifest decisions, not changes made by this guide.

Read [cancellation and terminal cleanup](file:///Users/peter/knowledge-bundles/harness-engineering/concepts/acp-tools-and-cancellation.md), [learner-owned async boundaries](file:///Users/peter/knowledge-bundles/harness-engineering/references/acp-rust-protocol.md), and canonical [v1 prompt cancellation](https://agentclientprotocol.com/protocol/v1/prompt-turn#cancellation).

## Diagnose before editing

Trace current `src/agent.rs::run` from its provider await at line 31 to `execute_tool_call` at line 67. Then inspect `src/tools.rs::run_bash_tool`, lines 120–137: blocking `Command::output()` owns no asynchronously cancellable child handle.

In your Lab 03/04 implementation, mark every await in `run_turn`, permission requests, and terminal create/wait/output/release. Identify the exact task that continues reading `session/cancel` while each wait is pending. If it is the waiting task itself, fix the dispatch structure first.

```sh
cargo test --locked acp_stage03_ -- --nocapture
cargo test --locked acp_stage04_ -- --nocapture
cargo test --locked -- --list
```

These source lines describe the pre-course baseline; use the named symbols and the new paths below after refactoring.

## Incremental learner actions

### 1. Define the turn lifecycle and a single owner

Create `src/acp/cancel.rs:1` for active-turn control and finalization helpers. Wire it from `src/acp/mod.rs` (or your existing `src/acp.rs` root, not both). Extend the Lab 03 session registry entry with a generation and cancellation handle; keep history in `src/session.rs`.

Use this **application design sketch**, not a full Rust implementation:

```text
Idle → Running(generation, signal, original-request-ID owner)
Running → Cancelling → CleaningUp → Finalizing → Idle
Running → CleaningUp → Finalizing → Idle             [ordinary finish]
any active state → Disconnected → local cleanup      [no deliverable reply]
```

Only the turn task/finalizer consumes the original prompt's response obligation. The cancellation notification route looks up the active generation, signals it, and resumes reading immediately. For unknown/idle sessions, the course policy is a redacted no-op; never reply to this notification or create a session.

**Rationale:** duplicate cancellation should be harmless, and the dispatch task must not wait for cleanup or take a lock held by the turn.

### 2. Make cancellation a competitor at each long wait

In `src/agent.rs::run_turn` and `src/provider.rs::OpenRouterProvider`, race provider completion against cancellation using your chosen signal and `tokio::select!`. Stop issuing new provider/tool calls once cancellation is observed. Use a fake provider that announces “started” and then waits on a channel.

This is a **control-flow sketch**, not paste-ready Rust:

```text
if signal is already cancelled: choose Cancelled
otherwise await first of provider reply / cancellation
if provider reply wins: validate reply, then re-check signal before side effects
if cancellation wins: stop polling owned provider future; join/stop owned workers
route cancellation-related errors to Cancelled, not ProviderFailure
```

Do not detach a provider worker then assume dropping its join handle stops it. Document the adapter's actual guarantee: local request work is stopped; this does not promise a remote service stopped computing or billing. **Rationale:** user-facing cancellation and remote execution guarantees are different claims.

### 3. Resolve permission races without granting authority

In `src/acp/tools.rs`, cancellation must end the permission wait even if a faulty client never sends its required cancelled outcome. Decode wire `outcome: {"outcome":"cancelled"}` into your own outcome enum and also handle the local signal. Resolve/remove the permission's entry in your bounded pending outbound request map and settle its oneshot exactly once. The router's reply delivery and cancellation cleanup must arbitrate entry ownership: if the reply already won, the turn still checks cancellation before execution. A late reply to a removed ID is ignored/logged, never a new authorization. Do not reuse IDs or leave dropped receivers registered forever.

Immediately before dispatching an approved effect, check the same turn generation and cancellation state. Define the local race rule: cancellation observed before effect dispatch wins; an already dispatched write cannot be promised undone. Report whether an effect was already applied rather than inventing rollback.

**Rationale:** [v1 clients must cancel pending approvals](https://agentclientprotocol.com/protocol/v1/tool-calls#requesting-permission), but an unresponsive peer must not hang the agent indefinitely. Baseline `session/cancel` addresses a session's turn; do not substitute a library-specific request-cancellation extension.

### 4. Clean up resources even if cancellation beats creation

Extend `TerminalLease` in `src/acp/tools.rs`. When a terminal ID is known, cancel the command with `terminal/kill`, collect final output if usable, and `terminal/release`; use bounded waits on every cleanup request. Do not reuse the already-cancelled turn signal to immediately skip cleanup.

Handle the harder race: cancellation while `terminal/create` is in flight. Keep an owner for the eventual create result; a late ID still needs release and must never become a new running tool. Transfer the pending correlation/oneshot receiver to a tracked cleanup task, retaining the same outbound ID and session/generation context. Do not apply the permission path's immediate-entry-removal rule to creation. Give the cleanup registry a finite capacity and deadline, and reserve admission for cleanup RPCs so pending ordinary work cannot starve kill/release. If no ID arrives before the deadline, explicitly report an unresolved remote creation; a bounded timeout does not prove nothing started.

If creation fails before an ID exists, there is nothing to release. If cleanup cannot be acknowledged, record the unresolved resource and fail the acceptance gate; do not claim successful teardown. On peer disconnect, client-terminal release cannot be confirmed over a dead connection—stop local work and report that boundary honestly.

**Rationale:** Rust `Drop` cannot await cleanup. Explicit finalization plus your tracked connection/session supervisor is required for task failure, shutdown, or abort. On EOF/writer failure, stop accepting work, settle pending oneshots, cancel/join turn and cleanup tasks with a bounded fallback, and clear owned registries without claiming delivery on a dead pipe. Kill/reap locally owned children independently of ACP transport availability. Dropping a Tokio join handle detaches work rather than stopping it.

### 5. Prove process termination, without silently enabling fallback

Keep ACP command execution delegated to client terminals. In `src/acp/test_support.rs`, implement a controlled fake terminal peer that can own one real child. The fixture starts a benign child in a temporary cwd, exposes its owned handle, and gates progress on a readiness line/channel; no provider chooses its command.

On kill/release, terminate and await the child. If you also retain a local CLI process executor, replace the blocking `src/tools.rs::run_bash_tool` path with an explicitly owned async child under proposed `src/local_process.rs:1`; that is a separate learner change, not an ACP fallback.

For an actual shell/descendant executor, killing only the shell is insufficient: define and test process-group/tree policy for the target OS. The first fixture can be a single process with no descendants; label that coverage honestly. **Rationale:** a fake release counter alone cannot prove no OS process remains.

### 6. Close the update stream, then complete once

In `src/acp/cancel.rs`, have the sole finalizer stop/join event producers, drain permitted final updates, restore valid session history, and enqueue the final response with the original request ID. Use the serialized bounded writer queue for every update and response; do not leave a second producer able to enqueue after the final boundary. Track write/flush failure separately from successful enqueue and propagate disconnect honestly. Clear the active generation without letting old cleanup erase a newer turn.

The following is an **illustrative wire pair**, grounded in [v1 cancellation](https://agentclientprotocol.com/protocol/v1/prompt-turn#cancellation), not a captured trace. Prompt ID 30 was already outstanding; the notification has no ID:

```json
{"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":"s1"}}
```

```json
{"jsonrpc":"2.0","id":30,"result":{"stopReason":"cancelled"}}
```

Define your own final-result DTO whose serialized field is `stopReason` and whose cancellation value is `cancelled`. Send it only after work is aborted and pending updates are settled, using ID 30 from the original prompt. Do not fabricate an acknowledgement on the cancel notification. If remote cleanup cannot be confirmed, report that limitation and fail the cleanup gate rather than presenting an unqualified successful teardown.

**Rationale:** `cancelled` is a normal prompt outcome, not an internal error. A cancellation arriving after completion must not rewrite that completed response. Define a single commit point for completion-vs-cancel races and test both orders.

## Deterministic tests to type

Put lifecycle tests under `#[cfg(test)]` in `src/acp/cancel.rs`; use `src/acp/test_support.rs` for a wire recorder, gated provider, permission peer, and terminal registry. Record Arrange / Act / Assert for every named case:

- **`acp_stage05_cancel_provider`** — Arrange provider-started and withheld-reply gates. Act by cancelling after started, never release a successful reply. Assert provider work stops, no new tool/model request occurs, and the original prompt completes once with cancelled within the test deadline.
- **`acp_stage05_cancel_pending_permission`** — Arrange a write approval waiting, with client reply withheld. Act with cancel, then a late allow-once reply. Assert no write before or after final completion, no later turn update, and bounded pending-request cleanup.
- **`acp_stage05_cancel_running_terminal`** — Arrange an acknowledged terminal ID with wait-for-exit pending. Act with cancel. Assert kill precedes release, final permitted updates precede the cancelled response, and the terminal registry is empty.
- **`acp_stage05_cancel_during_terminal_create`** — Arrange create requested but no ID yet. Act with cancel, then return an ID. Assert the late terminal is released, never reported as a successful tool, and finalization does not lose cleanup ownership.
- **`acp_stage05_duplicate_cancel_then_next_turn`** — Arrange a running turn. Act with two cancels, complete cleanup, start another prompt, and release an old worker's delayed event. Assert one first response, a fresh uncancelled next turn, and no stale update/busy-state mutation.
- **`acp_stage05_cancel_and_finish_races`** — Arrange two controlled schedules, cancellation-before-final-commit and completion-before-cancel. Act in each order. Assert cancelled in the first and the original normal result in the second; both have one response and no updates after it.
- **`acp_stage05_disconnect_and_cleanup_failure`** — Arrange disconnect during work, then separately a peer that times out on release. Act through teardown. Assert local tasks are stopped, no fabricated delivered reply/release success, and unresolved client cleanup is explicitly reported.
- **`acp_stage05_cancel_one_session_only`** — Arrange gated A and B. Act by cancelling A and completing B. Assert A cancelled, B normal, independent histories, and independent cleanup registries.
- **`acp_stage05_real_child_is_reaped`** — Arrange the controlled single-child terminal fixture and await its readiness signal. Act by cancelling its turn. Assert the owned child handle reports exit/reap, no later output arrives, the terminal is released, and the next turn works. Assert the handle, not just a potentially reused PID.
- **`acp_stage05_cancel_with_saturated_pending_map`** — Arrange ordinary outbound requests at their configured capacity and an active terminal. Act with cancel. Assert the router remains responsive, permission waiters settle, bounded cleanup admission allows kill/release, late approvals do nothing, and all owned entries are removed or explicitly reported unresolved by the cleanup deadline.

Add a bounded watchdog around every test. Prefer barriers/oneshot channels; use virtual time only for timeout policy, not to prove real OS scheduling. Record the expected cleanup order in the event log, not elapsed-time guesses.

```sh
cargo test --locked acp_stage05_ -- --list
cargo test --locked acp_stage05_ -- --nocapture
cargo test --locked acp_stage04_
cargo test --locked acp_stage03_
```

The real-child test must run in its disposable workspace with the fixed benign fixture. If unsupported on the host, mark the gate blocked rather than ignoring the test and claiming process cleanup. These tests are learner work, not tests already present.

## Acceptance gate

- [ ] Every named case is discovered and passes, including deterministic creation/approval/completion races.
- [ ] Cancellation is processed while provider, approval, and terminal waits remain pending.
- [ ] Exactly one response to the original prompt; none to the cancel notification; no updates after the response.
- [ ] Known terminals are released and the controlled real child is reaped; unresolved remote cleanup is a documented failure, not hidden success.
- [ ] Next turn and other sessions work; cancellation does not poison history, stale generations, or future approvals.
- [ ] In a separately authorized Zed scratch run, cancel once during model work, approval, and a command; inspect logs and actual process/terminal cleanup.

## Teach-back and evidence

Draw the task that reads cancel while the provider waits. Explain the difference between dropping a future, aborting a task, killing a command, and releasing a terminal. Describe the late-create-ID race and identify who still owns its cleanup. Show the assertion proving no updates occur after completion.

Record actual commands/results and limitations in [progress](progress.md) during review. A responsive spinner alone is not proof of cancellation or learner mastery.

**Snippet status:** state/control-flow blocks are design sketches and the JSON pair follows canonical v1 cancellation, not captured traffic. Historical SDK deserialization checks are superseded; they do not validate your DTOs/correlation/cleanup. No revised cancellation implementation, real process fixture, or Zed cancellation test was compiled/run during this documentation revision.
