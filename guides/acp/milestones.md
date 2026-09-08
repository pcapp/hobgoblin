# Development milestones — Rust agent to Zed

**Goal:** use your CodeCrafters Rust agent from Zed via stable ACP v1 over local stdio, while keeping the existing `-p` CLI. You write the application and tests; Hermes explains, inspects and verifies. This is a development sequence, not a claim that the features already exist.

**Architecture:** CLI and ACP are separate adapters around session-owned turn logic. The SDK owns protocol framing; your application owns conversation state, model calls, tool policy, outcomes and cleanup. Start without a model so protocol defects are not confused with credentials or provider failures.

**Stack:** existing Tokio, Clap, Serde, tracing and async-openai; add exact SDK pin `agent-client-protocol = "=2.1.0"` for the first lab. Add other crates only when a lesson needs them. See the wiki's [crate decision table](file:///Users/peter/knowledge-bundles/harness-engineering/references/acp-rust-sdk.md).

> **For Hermes:** this is a teaching plan, not permission to implement it with coding agents. Re-read `AGENTS.md`, the actual source/lockfile and `progress.md` before coaching. Do not edit application Rust unless Peter explicitly requests it.

## The sequence

| Milestone | Deliverable you build | Knowledge you earn | Gate before advancing |
|---|---|---|---|
| [00 — Inspect](00-inspect.md) | Boundary worksheet; no edits | Process/connection/session/turn, ownership and stdout | Explain the current code's gaps using source symbols |
| [01 — Initialize](01-handshake.md) | Standalone typed handshake | Builders, responders, version/capability negotiation | Six no-key subprocess probes pass; explain each |
| [02 — Echo in Zed](02-echo.md) | `--acp` startup, session store, updates, slow-echo cancellation | CLI routing, ID ownership, state machines, ordered dispatch | Parser/process/session tests; two Zed threads echo independently |
| [03 — Run your agent](03-turn-engine.md) | Session-owned history and adapter-independent turn engine | Borrowing across awaits, typed outcomes, provider test doubles | Deterministic multi-turn/isolation/error tests; CLI preserved |
| [04 — Expose safe tools](04-tools.md) | Tool reporting, permission gates, capability-aware execution | Approval versus capability; filesystem/terminal lifetime | Denial has zero side effects; cwd, exit and cleanup tests |
| [05 — Stop reliably](05-cancellation.md) | Cancellation across provider, approval and commands | Cooperative cancellation, task/resource ownership, races | Bounded cancellation, exactly-once completion, no orphan work |
| [06 — Prove the supported profile](06-compatibility.md) | Baseline resource links + stdio MCP, negative-path matrix, Zed evidence | Conformance versus demo; reproducible compatibility claims | Deterministic suite plus redacted end-to-end evidence for a named Zed/version profile |

Milestone 00 is a short preflight, not a week of theory. Milestone 01 is your first coding session. Break later milestones at their checkpoints; “complete one test and one behavior” is a useful session goal. Do not estimate mastery from time spent or checkboxes.

## Start here: initialization

Read [what initialization means](file:///Users/peter/knowledge-bundles/harness-engineering/concepts/acp-initialization.md), then do the Lab 00 source inspection and [Lab 01](01-handshake.md).

Your first finish line is deliberately small:

- Start a process with no model key.
- Send an `initialize` request.
- Receive one JSON-RPC response with the same request ID and supported protocol v1.
- Keep human text off stdout.
- Explain why a handshake creates neither a session nor an answer.

**Not today:** a model API call, tool execution, MCP server installation, streaming tokens, a generic plugin framework, remote HTTP transport or a wiki frontend.

## Repeat this learning loop at every step

1. **Predict:** say what behavior is missing and what the test should observe.
2. **Inspect:** read the named source symbol and pinned API page; re-find moved code.
3. **Red:** write the smallest named test and run it. A missing binary/test target proves setup is incomplete, not that the behavioral assertion works.
4. **Implement:** make one change yourself. Short API examples illustrate mechanisms; they are not an invitation to paste an entire finished agent.
5. **Green:** run the focused test, then regressions. Check that tests were actually discovered.
6. **Explain:** what changed, who owns the state/resource, and what can still fail?
7. **Review:** ask “Verify ACP milestone NN.” Hermes checks the diff and runs agreed no-cost tests; record evidence in [progress.md](progress.md).

Commit a checkpoint when you understand and have verified it. Review the diff and stage your intended files explicitly; this repository already contains learning-material changes. Do not use `git add .` blindly, and do not ask Hermes to commit unless you want it to.

## Test layers and what they prove

| Layer | Typical test | What it cannot prove |
|---|---|---|
| Pure unit | Session store rejects duplicate active turns; outcome mapping | Process startup/framing |
| Scripted provider/client | Known response/error; delayed approval; cancellation barrier | Real provider or Zed compatibility |
| Subprocess wire | Real stdin/stdout, request correlation, interleaved notifications | UI rendering or unsaved editor state |
| Disposable workspace | Denied write, command exit, process cleanup | Arbitrary-host security guarantees |
| Zed smoke test | Echo, same-session context, approval, cancel | Every ACP client's compatibility |

Use offline/scripted responses for repeatable behavioral tests. A live model remembering “cedar” is useful smoke evidence, not a deterministic regression test. Real model calls can cost money; real tools and MCP launches execute code. These require an agreed bounded test scope, never an automatic jump from the echo lesson.

## Common commands

Run these from `/Users/peter/repos/codecrafters-claude-code-rust`, **not the wiki**:

```sh
git status --short
git diff -- src Cargo.toml Cargo.lock
cargo check --locked
cargo test --locked
```

`status --short` shows tracked/untracked changes. `diff --` limits the view to the named paths; inspect new untracked files separately. `check` type-checks; `test` compiles and runs registered tests; `--locked` forbids lockfile updates. After intentionally adding a dependency, first run the lesson's unlocked Cargo command and inspect its lockfile change, then return to locked checks.

As tests grow, run a named test first: `cargo test --locked TEST_NAME -- --nocapture`. `TEST_NAME` is a substring filter you replace with the actual test name; it can accidentally match **zero tests**. That is not a passing milestone. Lessons name integration targets only after instructing you to create them.

`cargo fmt --all -- --check` is a non-mutating formatting check; compare against the baseline rather than attributing unrelated old formatting to your patch. Avoid `run.sh` for automatic checks: it currently enables a proxy and invokes a live model/tool prompt.

## What to hand Hermes

Use this in chat; keep durable observations in the existing ledger rather than creating another progress store:

```text
Verify ACP milestone:
Changed files/symbols:
My prediction and explanation:
Focused test command + observed result:
Regression command + number of tests discovered:
Redacted transcript/report path:
Known failure or question:
```

Hermes reviews correctness, negative cases, capability honesty, stdout purity, CLI regressions and resource cleanup relevant to the milestone. Missing tests or uncertain results remain open. Assistant-run guide checks are recorded separately from learner evidence.

## Current validation versus future work

The existing repository builds but currently discovers no Rust tests. Application features above are assignments, not implemented adapters. The supplied [handshake verifier](check_handshake.py) is ready to use after you build your example. Guide validation details and actual probe frames are in [milestone-validation.json](milestone-validation.json) and [handshake-validation.json](handshake-validation.json); their temporary executable path is historical evidence, not your install path.

No learner mastery, real Zed conversation, paid model call or real tool/MCP side effect is established by generating these guides.

[Course home](README.md) · [Progress ledger](progress.md) · [Wiki learning workflow](file:///Users/peter/knowledge-bundles/harness-engineering/workflows/acp-learning.md)
