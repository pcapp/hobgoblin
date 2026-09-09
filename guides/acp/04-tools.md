# Lab 04 — Visible tools without invisible authority

**User-visible milestone:** Zed shows a tool card, asks before a write/command, displays its result, and does nothing when approval is denied. Reads can see unsaved editor text; failed commands do not appear successful.

**Status:** generated coursework. Peter implements the proposed changes; no real side effect is authorized merely by reading this lesson.

[Course](README.md) · [Progress](progress.md) · [Previous: turn engine](03-turn-engine.md) · [Next: cancellation](05-cancellation.md)

## Prerequisites

- Pass Lab 03's scripted-provider, session isolation, CLI regression, and response-ordering gates.
- Retain the stage 02 cancellation path. Approval waits already require a responsive connection; full interruption is exercised in Lab 05.
- Work only in a disposable workspace you create for this lab, not the repository or personal files.
- Keep stable ACP v1 with learner-owned protocol DTOs and no ACP agent/schema SDK dependencies. Reuse Tokio, Serde/serde_json, `schemars` for existing tool argument schemas, and tracing.

## Concepts and crate choices

Reporting, permission, and execution are separate responsibilities. A capability says the client implements a method; it is not approval. A model-requested tool name is untrusted input, not authorization.

Use a typed application `ToolResult`, an async executor boundary, and a separate ACP presenter. Keep argument schemas in `src/tools.rs`; reuse the existing provider schema format. Define your own request/result/update DTOs from canonical ACP fields and serialize with Serde; do not import an ACP schema crate. Tokio channels support correlation and the fake peer; retain direct `io-util`/`io-std`/`sync` features from Lab 02 and `time` for bounded requests.

For this course, **local policy** requires allow-once approval for writes and commands, constrains reads to the session workspace, and fails unavailable client operations explicitly. There is no silent fallback to `std::fs` or local `bash` in ACP mode. CLI may retain its explicit local executor, clearly separated from ACP policy.

Read [tool safety](file:///Users/peter/knowledge-bundles/harness-engineering/concepts/acp-tools-and-cancellation.md), [API directions/capabilities](file:///Users/peter/knowledge-bundles/harness-engineering/protocols/acp-api-map.md), and [learner-owned routing/correlation](file:///Users/peter/knowledge-bundles/harness-engineering/references/acp-rust-protocol.md).

## Diagnose before editing

| Current target | What to demonstrate by inspection |
|---|---|
| `src/tools.rs:11–25`, `ReadArgs`, `WriteArgs`, `BashArgs` | JSON arguments are decoded separately for each tool. |
| `src/tools.rs:58–60`, `specs` | All tools are exposed together regardless of client capabilities. |
| `src/tools.rs:62–106`, `read_file_tool`, `write_file_tool` | Direct disk I/O cannot see unsaved editor buffers and has no approval boundary. |
| `src/tools.rs:108–137`, `run_bash_tool` | Blocking `.output()`, inherited process cwd, byte-array JSON, and no exit-status check. |
| `src/tools.rs:140–150`, `execute_tool_call` | Synchronous dispatcher chooses tools; unknown names return an error. |
| `src/agent.rs:66–77`, `execute_tool_call` invocation | The loop consumes raw JSON instead of an explicit execution result. |

Re-find symbols if Lab 03 moved them. Explain from the current Bash implementation why “spawn succeeded” cannot prove “command succeeded”; do not run an uncontrolled model-generated command to demonstrate the defect.

```sh
cargo test --locked acp_stage03_ -- --nocapture
cargo test --locked -- --list
git diff -- src/tools.rs src/agent.rs
```

## Incremental learner actions

Use `src/acp/mod.rs` as the adapter root below, or the equivalent stage 02 `src/acp.rs`; do not create two module roots. Child paths remain `src/acp/…`.

### 1. Separate parsing from side effects

In `src/tools.rs`, extract argument validation from `read_file_tool`, `write_file_tool`, and `run_bash_tool`. Retain `Read`/`Write`/`Bash` names and schemas. Introduce a validated invocation and typed result; make `execute_tool_call` dispatch to the selected executor rather than immediately invoking disk/process operations.

**Domain design sketch**, not a wire definition or full implementation:

```rust
pub enum ToolResult {
    Text(String),
    Command { output: String, exit_code: Option<i32>, signal: Option<String>, truncated: bool },
    Failed { message: String },
}
```

Map this result into both model tool messages and UI status. Command exit code zero with no terminating signal is success; nonzero/signal is failure. Missing status means unknown/running, never presumed success. For the local CLI path, intentionally decode output bytes (for example, lossy UTF-8 with a documented replacement policy), retaining separate stdout/stderr if useful; ACP terminal output is already a string.

**Rationale:** transport success and business outcome must not collapse into one `ok: true`.

### 2. Build capability and workspace checks first

Create `src/acp/tools.rs:1` for the ACP executor and `src/workspace.rs:1` for the local workspace policy. In the initialization route in `src/acp/mod.rs`, retain **client capabilities from the request**, not from your response. Filter the provider tool catalog per session, and still validate every requested tool defensively.

Store the absolute `session/new.cwd` in `SessionState` from Lab 03. Resolve relative paths against it, never `std::env::set_current_dir`. Reject traversal and disallowed absolute paths. For existing paths, consider canonical ancestors/symlinks; for new files, validate the existing parent and final component. Document TOCTOU limitations: this is not an OS sandbox, and client/local filesystem views can differ.

Keep `fs.readTextFile` and `fs.writeTextFile` gates independent. No capability means no corresponding request; it does not permit a more powerful local fallback. **Rationale:** two sessions and an editor buffer must not share accidental process-global authority.

### 3. Report intent, request permission, then execute

In `src/acp/tools.rs`, allocate a tool-call ID unique within the session (include a turn sequence if provider IDs can repeat). Preserve the separate provider ID for model tool-result correlation. Send a pending tool card before permission; use the same ACP ID for every update and approval request.

For writes/commands offer only `allow_once` and `reject_once` initially. Match the returned **option ID** against the exact offered options; unknown IDs, cancelled outcomes, peer errors, and missing replies never authorize execution. Re-check active-turn cancellation immediately before the side effect.

The following **illustrative wire fixture**, grounded in [v1 permissions](https://agentclientprotocol.com/protocol/v1/tool-calls#requesting-permission), is not a captured exchange. Define the request/outcome DTOs yourself; ID 50 is agent-allocated, unrelated to the outstanding client's prompt ID or the tool-call ID.

```json
{"jsonrpc":"2.0","id":50,"method":"session/request_permission","params":{"sessionId":"s1","toolCall":{"toolCallId":"t1-write","title":"Write scratch.txt","kind":"edit","status":"pending"},"options":[{"optionId":"allow-once","name":"Allow once","kind":"allow_once"},{"optionId":"reject-once","name":"Reject","kind":"reject_once"}]}}
```

```json
{"jsonrpc":"2.0","id":50,"result":{"outcome":{"outcome":"selected","optionId":"reject-once"}}}
```

A selected reject is not a cancelled prompt. Return a denied tool result to the model and mark the tool failed with an explanation. A cancelled approval means the turn was cancelled; stop work and complete through the cancellation path.

Use the pending outbound request map introduced in Lab 02, not an await inside the reader/router. In your own request operation: reserve a bounded slot, allocate a non-reused ID, register its method/session/generation context and oneshot sender, enqueue the serialized request, then await the receiver in the turn task. The router validates a reply, removes exactly its entry, and delivers either result or error. Validate method-specific result DTOs before trusting them. No map lock may span an enqueue or reply await.

On enqueue failure, timeout, cancellation, or disconnect, settle/remove the entry and wake its waiter with a typed failure; dropping a receiver alone does not remove a map entry. Never reuse timed-out IDs. Late/unknown/duplicate replies cannot authorize work and receive no response. If cancellation races with a resource-creating request, transfer its correlation to a bounded tracked cleanup owner rather than losing a late terminal ID (Lab 05). Bound both map size and each wait; reserve cleanup capacity or another bounded admission path so a full map of ordinary requests cannot prevent kill/release.

**Rationale:** waiting for a permission oneshot is safe in an independently running turn task only because the reader/router continues to deliver replies and cancel signals. Send `in_progress` only after validation/approval; finish with `completed` or `failed`. Canonical v1 tool statuses have no wire `cancelled` variant; cancellation is a prompt stop reason.

### 4. Delegate editor-aware file access

Implement Read/Write routes in `src/acp/tools.rs` using `fs/read_text_file` and `fs/write_text_file`, with validated absolute paths and the session ID. Read canonical [filesystem methods](https://agentclientprotocol.com/protocol/v1/file-system) and [wire schema](https://agentclientprotocol.com/protocol/v1/schema), then define only the DTOs needed. Route each reply by its own ID and validate the success/error envelope before decoding content. Where examples disagree, record the discrepancy and use the canonical schema; follow initialization's request placement for client capabilities, not the filesystem page's contradictory response example.

A read result comes from the client, including unsaved edits where supported. A successful approved write must be checked in the fake client's file store and later read back in Zed; the tool card alone is not proof. Bound content sizes and redact logs. **Rationale:** editor filesystem semantics are part of the integration, not an interchangeable local read optimization.

### 5. Make terminal ownership explicit

Implement Bash via `terminal/create` with explicit `command`, `args`, absolute session `cwd`, environment policy, and output byte limit. If preserving shell syntax, deliberately use `/bin/bash` with `-c` and present that command for approval; do not pretend arbitrary shell text is a safe argv list.

Use [v1 terminal lifecycle](https://agentclientprotocol.com/protocol/v1/terminals) to implement this **design sequence**:

```text
approved → create → save terminal ID → report terminal content
         → wait_for_exit → output → classify exit → release
error/cancel after create → attempt kill when needed → output if usable → release
```

Create `TerminalLease` in `src/acp/tools.rs` owning session/terminal IDs and cleanup state. Fetch output before release; the ID is invalid afterward. Attempt release after wait/output errors too. `Drop` cannot await RPC, so normal paths need explicit async cleanup; Lab 05 covers cancellation and disconnect supervision.

**Rationale:** returning a terminal ID is not command completion; `kill` is not `release`. Follow initialization's canonical request capability placement despite contradictory terminal-page examples.

## Deterministic tests to type

Put tests in `src/acp/tools.rs` under `#[cfg(test)]`; reuse `src/acp/test_support.rs` from Lab 03 for a fake peer, file store, request log, and terminal registry. Use no real shell in these tests.

- **`acp_stage04_denied_write_has_no_effect`** — Arrange sentinel content and reject-once permission. Act with Write. Assert unchanged store, zero write RPCs, pending→failed on one tool ID, and a denied result available to the provider.
- **`acp_stage04_allow_once_does_not_authorize_next_write`** — Arrange two writes with first allowed and second rejected. Act sequentially. Assert two distinct permission requests, one write RPC, and only the first content change.
- **`acp_stage04_unknown_permission_fails_closed`** — Arrange an unoffered option ID and, separately, permission RPC failure. Act with Write. Assert no execution in either case and a useful failed result.
- **`acp_stage04_client_read_sees_unsaved_buffer`** — Arrange disk text “old”, client store “edited”, and read capability. Act with Read. Assert result is “edited”, one read RPC uses the session path, and local disk was not read.
- **`acp_stage04_missing_capabilities_do_not_fallback`** — Arrange table cases with missing read/write/terminal support, including read-only support. Act with each unsupported tool. Assert no forbidden RPC and no local executor invocation.
- **`acp_stage04_cwd_and_escape_checks`** — Arrange two temporary roots and controlled `..`, symlink escape, prefix-lookalike, and new-file cases. Act on validated paths. Assert each relative target belongs to its own root, escapes are denied, and process cwd never changes.
- **`acp_stage04_nonzero_terminal_exit_is_failure`** — Arrange create→ID, exit code 7, textual output and truncation flag. Act with approved Bash. Assert failed tool status, preserved exit/output/truncation in model result, and exactly one successful release.
- **`acp_stage04_terminal_error_still_releases`** — Arrange a valid terminal ID then wait/output error in separate cases. Act through cleanup. Assert a release attempt in both, no completed tool status, and no later request using a released ID.
- **`acp_stage04_tool_ids_correlate_without_collision`** — Arrange repeated provider call IDs in two turns. Act through approval and result. Assert ACP IDs differ across turns but remain stable within each lifecycle; provider tool messages retain their original IDs.
- **`acp_stage04_reverse_replies_are_correlated`** — Arrange concurrent permission/read requests, an inbound prompt with an equal numeric ID, and replies delivered out of order. Act through the real reader/router. Assert each waiting task receives only its own typed result/error and the inbound response obligation remains independent.
- **`acp_stage04_pending_requests_are_bounded`** — Arrange a full map, enqueue failure, timeout, duplicate/unknown reply, and disconnect as separate cases. Assert bounded admission, all abandoned waiters settled/entries removed, no new authorization from late replies, no response-to-response loop, and cleanup requests can still make progress.

Drive every reply through the real asynchronous peer route, not just a direct permission function call, so the test would catch dispatch deadlock. Use explicit started/reply gates and bounded timeouts, not sleep-based timing.

```sh
cargo test --locked acp_stage04_ -- --list
cargo test --locked acp_stage04_ -- --nocapture
cargo test --locked acp_stage03_
cargo check --locked
```

These are proposed tests to implement, not existing passing tests. Confirm every name is discovered and executes before counting the gate as passed.

## Acceptance gate

- [ ] Named deterministic tests pass; denied/invalid/cancelled approval cannot execute.
- [ ] Tool card IDs, provider IDs, and permission request IDs are distinguishable in one annotated trace.
- [ ] No absent-capability local fallback; cwd isolation and path-policy limitations are explicit.
- [ ] Command nonzero exit/signal cannot produce a completed tool card; terminals are released on normal/error paths.
- [ ] In Zed, use a scratch file: deny a write and read back unchanged content; approve another and read back the change. Trigger via a controlled fixture or an explicitly approved live model run.
- [ ] Verify an unsaved scratch buffer is read through the client; retain only redacted observations, not secrets or full prompts.

## Teach-back and evidence

Explain why a supported write still needs approval, why “selected” is not always “allowed”, why `kill` leaves a resource to release, and why local disk might disagree with the editor. Point to the test proving a side effect never happened rather than merely proving an error was displayed.

Record actual attempts/results in [progress](progress.md) during review. Do not label the generated design as demonstrated understanding.

**Snippet status:** the Rust result is a domain sketch and the permission fixtures follow canonical v1 wire fields. Historical SDK deserialization checks are superseded, not validation of your DTOs or router. No revised executor or tests were compiled/run during this documentation revision. Client-terminal and Zed behavior remain unverified until exercised.
