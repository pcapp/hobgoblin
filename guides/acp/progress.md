# ACP learning progress

## Selected working agreement

- Target: Zed, local stdio; preserve the existing CLI.
- Reusable concepts: existing OKF harness-engineering wiki.
- Repo: exercises, source-specific observations, and evidence.
- Viewer: Obsidian now; a website later.
- Teaching: experienced-engineer pace; explain relevant Rust; Peter writes application code.
- Tracking: during working sessions, not background monitoring.

## Learner evidence

No ACP mastery has been assessed yet. Existing `LEARNING_LOG.md` entries show prior topics, not proof of current ACP proficiency. Writing this course and running its example do not advance these states.

| Concept | State | Learner explanation / evidence | Next exercise |
|---|---|---|---|
| Agent/client/provider/MCP boundaries | not_started | — | Lab 00 teach-back |
| Request ID vs session ID vs tool-call ID | not_started | — | Annotate an exchange |
| JSON-only stdout and framing | not_started | — | Labs 00–01 |
| SDK roles, builders, typed callbacks | not_started | — | Lab 01 |
| Session-owned history and Rust ownership | not_started | — | Stages 02–03 |
| Updates versus prompt completion | not_started | — | Echo into Zed |
| Dispatch concurrency and cancellation | not_started | — | Slow echo, then stage 05 |
| Capability versus permission | not_started | — | Deny a scratch write |
| File/terminal semantics and cleanup | not_started | — | Stage 04 |
| Baseline resource links and stdio MCP | not_started | — | Stage 06 |

States: `not_started` → `introduced` → `practiced` → `demonstrated`; use `revisit` when a later task exposes a gap. Promotions require actual learner engagement/evidence.

## Environment and guide validation — assistant-run, not learner mastery

Recorded at 2026-09-08T17:32:12+00:00.

- Repository began clean on `master`.
- `rustc 1.98.0`; Cargo `1.98.0`; installed Zed `1.17.2`.
- `cargo check --locked`: passed on the existing repo.
- `cargo test --locked`: passed, **0 tests discovered**. No behavioral coverage claimed.
- SDK baseline: published `agent-client-protocol 2.1.0`, schema dependency `1.7.0`, protocol v1.
- The exact Lab 01 Rust snippet compiled in an isolated temporary Cargo project using the pinned SDK.
- Real stdin/stdout probes requested protocol versions 1 and 2. Each process exited 0, emitted one JSON response with matching request ID and selected version 1, and emitted no stderr. This checks the handshake only.
- No project `src/`, manifest, or lockfile was changed by authoring/validating the guide.
- No Zed session, model request, real tool side effect, or MCP interoperability test has been run.

## Milestone guide validation — assistant-run, not learner mastery

Recorded at 2026-09-08T21:38:15.818354+00:00.

- Expanded the [milestone roadmap](milestones.md) and dedicated lessons 01–06; Lab 00 remains the short inspection preflight.
- Rechecked repository `master`: pre-existing `LEARNING_LOG.md`/guide changes were present. Application source, `Cargo.toml` and `Cargo.lock` are unchanged by this work.
- Existing `cargo check --locked` and `cargo test --locked` pass; **0 application tests discovered**. The new Rust tests in lessons are assignments, not present in the application.
- Exact Lab 01 example compiled in an isolated crate with SDK 2.1.0/schema 1.7.0. The supplied `check_handshake.py` passed all six real no-key/stdout probes. Deliberately faulty banner, version and hung-process fixtures were rejected.
- Lab 02 snippet checks: four argument-parser cases and one typed-payload case passed in isolation. Later permission/cancel/prompt JSON examples passed five typed deserialization tests; Rust enum/API sketches compiled.
- See [handshake-validation.json](handshake-validation.json) for actual wire frames and [milestone-validation.json](milestone-validation.json) for overall scope. Temporary validation binaries are not installed application features.
- No Zed interoperability, model call, real tool side effect, or MCP integration was exercised. Learner states above remain unchanged.

## Session evidence template

Copy for each actual lesson; never fill answers on Peter's behalf.

```text
When (offset-aware datetime):
Concept / exercise:
Starting state:
Peter's explanation or attempted solution:
Evidence: command or artifact path + observed result (redacted):
Feedback / correction:
New state and why:
Remaining uncertainty:
Next small exercise:
```

## Current next action

Complete [Lab 00](00-inspect.md): explain why the current local `messages` vector and direct `println!` are separate ACP integration problems. Then type [Lab 01](01-handshake.md), run its supplied handshake verifier and ask “Verify ACP milestone 01.” See [milestones](milestones.md) for the full sequence.

[Course](README.md) · [Existing learning notebook](../../LEARNING_LOG.md)
