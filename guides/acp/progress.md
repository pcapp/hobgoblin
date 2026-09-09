# ACP learning progress

## Selected working agreement

- Target: Zed, local stdio; preserve the existing CLI.
- Reusable concepts: existing OKF harness-engineering wiki.
- Repo: exercises, source-specific observations, and evidence.
- Viewer: Obsidian now; a website later.
- Teaching: experienced-engineer pace; explain relevant Rust; Peter writes application code.
- Implementation boundary: Peter explicitly rejected the ACP agent SDK. Serde and Tokio are fine; Peter implements ACP framing, wire types, dispatch and correlation himself. No ACP agent/schema SDK dependency.
- Tracking: during working sessions, not background monitoring.

## Learner evidence

Lab 00 written answers have been reviewed against the current source. The states below distinguish Peter's attempted explanations from corrections supplied during review. Existing `LEARNING_LOG.md` entries and assistant-run checks are not proof of current ACP proficiency.

| Concept | State | Learner explanation / evidence | Next exercise |
|---|---|---|---|
| Agent/client/provider/MCP boundaries | not_started | — | Lab 00 teach-back |
| Shared core and terminal/ACP frontends | introduced | Peter requested the architecture explanation and asked to make it the first lesson; no design teach-back or implementation evidence yet | Lab 00 diagram, ownership map and first-refactor test plan |
| Request ID vs session ID vs tool-call ID | practiced | Lab 00 diagram separates request IDs and session IDs; session typo and reversed final response remain; tool-call ID unassessed | Correct exchange and add permission round trip |
| JSON-only stdout and framing | practiced | Correctly identifies raw answer stdout vs tracing stderr and ACP JSON-RPC requirement | Verify newline framing in Lab 01 |
| Handwritten framing, envelope validation and dispatch | not_started | SDK-based objective replaced at Peter's request; no handwritten implementation evidence yet | Revised Lab 01 |
| Session-owned history and Rust ownership | practiced | Correctly identifies local history loss; teach-back proposes file/database persistence but has not distinguished in-memory session lifetime from restart durability | Explain why an in-memory session map suffices across turns |
| Updates versus prompt completion | practiced | Includes both update and stop reason in Lab 00 diagram, but completion arrow is reversed | Correct response direction and notification ID distinction |
| Dispatch concurrency and cancellation | practiced | Teach-back identifies a waiting stdin reader; independent dispatch and signalling the active turn remain to explain | Explain why reading bytes alone is insufficient |
| Capability versus permission | practiced | Teach-back suggests failure reason, OS permissions and missing files; user denial vs filesystem failure and absence of side effects need clarification | Assert denial leaves an existing file unchanged and never invokes the writer |
| File/terminal semantics and cleanup | practiced | Identifies Output and Vec<u8>; command-exit handling, JSON byte representation and cwd remain gaps | Explain nonzero exit, byte arrays and session cwd |
| Baseline resource links and stdio MCP | not_started | — | Stage 06 |

States: `not_started` → `introduced` → `practiced` → `demonstrated`; use `revisit` when a later task exposes a gap. Promotions require actual learner engagement/evidence.

## Environment and guide validation — assistant-run, not learner mastery

**Historical scope:** this section and the following milestone-validation section describe the superseded SDK-based guide. Their compiled snippets and reports do not validate the handwritten replacement. Keep the recorded observations intact; learner states do not advance from the rewrite.

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

## Lab 00 written-answer review — 2026-09-08T16:00:47-07:00

- Evidence: Peter's answers and Mermaid diagram in [00-inspect.md](00-inspect.md), sections 2–5. Reviewed against `src/agent.rs`, `src/tools.rs`, `src/main.rs` and the wiki API map on clean `master` before this ledger update.
- Correct observations: each run owns independent local history; the client is borrowed; human answer output conflicts with protocol stdout; malformed provider data, empty choices and loop exhaustion return `Ok(())`.
- Corrections supplied: locked serde_json 1.0.151's `json!` borrows interpolated value expressions (`to_value(&$other)`); command `.output()` success is distinct from `output.status.success()`; current Bash output ignores exit status and serializes byte vectors as JSON arrays. Tool error JSON is not an `Err` propagated from `run`.
- Unanswered topics: cancellation must reach responsive protocol dispatch while turn work waits; synchronous `.output()` blocks its executing thread. Commands currently inherit process cwd; session-specific cwd should be configured per child rather than changing global process cwd.
- Diagram feedback: final response must flow agent → Zed and match prompt request ID 3; keep session ID spelling consistent; add an agent → client permission request and its correlated response before prompt completion. Updates are notifications without request IDs.
- No new build, test, model request or runtime cancellation check was performed. No application source or learner answers were changed. States remain `practiced`; corrections are not yet learner teach-back evidence.
- Next: revise the Bash-success answer and diagram, then answer the four checkpoint questions before moving to Lab 01.

## Lab 00 conversational teach-back — 2026-09-08T16:11:14-07:00

- Peter proposed storing each session in a database or file, emphasizing persistence. Feedback: state must outlive a turn, not necessarily the process; an in-memory session-ID map suffices initially, while disk persistence supports restart/resume.
- Peter proposed a CLI mode switch and interface-like output dispatch to JSON-RPC or `println!`. This demonstrates the presentation-boundary design in explanation, not a working implementation. Feedback: CLI and ACP are frontends; Rust traits can express the interface, but dynamic dispatch is optional and the ACP SDK should handle wire serialization.
- Peter identified stdin/stdout transport and a waiting reader. Feedback: dispatch must continue processing cancellation and signalling active work while a turn is pending; a reader alone does not guarantee responsiveness.
- Peter suggested asserting a specific failure reason, mentioning OS permissions or a missing file. Feedback: user denial is an authorization decision before execution; test that a writable existing file remains unchanged and the write executor is not called.
- No code or runtime checks were performed. Broader concept states remain `practiced`; the supplied corrections are not learner mastery evidence.
- Next small exercise: explain the result of a denied overwrite of a writable file, including the expected executor call count and file contents. Then close remaining Lab 00 gaps before the handshake lab.

## Course boundary correction — 2026-09-08T16:49:18-07:00

- Peter: “I do not want to use the SDK. Please remove it.” Clarification: “Serde and Tokio are completely fine. It's the agent SDK I don't want to use.”
- Replaced the SDK-based Lab 01 with learner-owned framing, validation, dispatch, correlation and serialization exercises. Lessons 02–06 and the wiki now retain that boundary rather than reintroducing SDK callbacks.
- Earlier assistant guidance that the ACP SDK should own serialization is superseded. The original conversational record above remains historical evidence, not the current implementation agreement.
- Live inspection on `master`: the project has no ACP agent SDK in its source, manifest or resolved dependency graph, so no dependency removal or application edit was needed. Existing modified guide/progress files were preserved.
- Baseline `cargo check --locked` and `cargo test --locked` passed; the latter discovered **0 tests**. This is existing-project build evidence only, not a passing handwritten handshake or learner mastery.
- Final documentation verification: [SDK-free course validation](sdk-free-course-validation.json) records successful course/wiki structure, JSON fixture and local-link checks, no active SDK API prescriptions, and no application/dependency changes. The uv checker launches from the wiki using absolute paths; all six probes still report the expected missing learner binary. No handwritten handshake or Zed run is claimed.
- Next: implement the revised Lab 01 in small steps. Outstanding Lab 00 explanations remain open; the course-direction decision is not a mastery promotion.

## Architecture-first course orientation — 2026-09-08T17:18:11-07:00

- Peter asked how to structure interactive and ACP modes, then requested that the shared-core/two-frontends explanation become the first lesson.
- Added [Lab 00 — One agent, two frontends](00-architecture.md) before the handshake. The existing [inspection worksheet](00-inspect.md) and learner answers remain in place.
- Lesson deliverables are an architecture diagram, source-linked responsibility/ownership notes, and a deterministic plan for caller-owned history and presentation-free answers. The core refactor remains implementation work in Lab 03; no application changes were made to author this lesson.
- State is `introduced` from actual discussion, not `practiced` or `demonstrated`. Favorable feedback on the explanation is not a completed exercise.

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

Start [Lab 00 — One agent, two frontends](00-architecture.md): map the shared core, session ownership, output and tool boundaries to the existing source. Complete the remaining [inspection worksheet](00-inspect.md) explanations, then continue to [Lab 01](01-handshake.md). See [milestones](milestones.md) for the full sequence.

[Course](README.md) · [Existing learning notebook](../../LEARNING_LOG.md)
