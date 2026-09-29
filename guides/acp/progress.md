# ACP learning progress

This ledger tracks the current hands-on task sequence. The previous architecture-first and standalone-example course has been retired.

## Working decisions

- Evolve the existing application; do not build a separate example agent.
- One shared core supports one-shot CLI, interactive CLI, and ACP frontends.
- Keep `-p` as one-shot mode.
- Running without arguments starts an interactive multi-turn conversation.
- `--acp` starts the ACP frontend.
- Implement ACP directly with Serde and Tokio.
- Do not use an ACP SDK or external ACP schema crate.
- Use high-level, goal-driven tasks with machine-verifiable acceptance checks.
- The learner owns the design and writes the Rust.

## Task status

| Task | Status | Goal | Evidence |
|---|---|---|---|
| [1 — Shared core](01-shared-core.md) | complete | Caller-owned history; returned answer; `-p` preserved | Four deterministic behavioral tests pass; all acceptance commands exited 0 on 2026-09-29 |
| [2 — Interactive CLI](02-interactive-cli.md) | not started | Multi-turn terminal conversation; `/exit`, `/quit`, and EOF | — |
| [3 — ACP initialize](03-acp-initialize.md) | not started | Real binary completes ACP initialization through Tokio stdio | — |

Use `not started`, `in progress`, `complete`, or `blocked`. Mark a task complete only when every listed command exits with status 0 and its required automated cases are present in the test/checker output.

## Session checkpoint — 2026-09-29

### Current understanding

- `Conversation` owns the ordered provider-visible message history and keeps its messages private.
- The terminal frontend owns each conversation; `agent::turn` temporarily borrows it as `&mut Conversation` and retains user, assistant, and tool messages.
- `agent::turn` returns final assistant text rather than printing it, allowing each frontend to choose its presentation mechanism.
- `terminal::run_once` accepts a `Write` output boundary, so production can use stdout while tests use an in-memory `Vec<u8>`.
- A small `Model` trait allows deterministic scripted tests without network requests or credentials.

### Source checkpoint

- Task 1 is complete and verified.
- `src/agent.rs` contains the caller-owned conversation boundary and three agent behavior tests.
- `src/terminal.rs` contains one-shot and interactive entry points plus the one-shot output test.
- `src/main.rs` passes stdout to one-shot mode.
- The working tree has uncommitted changes in `src/main.rs`, `src/terminal.rs`, and this ledger.

### Exact resume point

1. Read `02-interactive-cli.md` and compare its acceptance criteria with the existing `run_interactive` scaffold.
2. Verify the current EOF behavior before changing code.
3. Introduce only the input/output seams needed for deterministic interactive tests.
4. Implement `/exit`, `/quit`, EOF, and multi-turn behavior one requirement at a time.

Rust principles practiced in Task 1: model state with a struct, preserve invariants with private fields, express temporary mutation with `&mut`, return data instead of performing caller-specific effects, use trait bounds at genuinely variable boundaries, and inject `Write` for testable output.

## Validation record

Add one structured entry after validation:

```text
Date: 2026-09-29
Task: 1 — Shared core
Commit or tree state: master; uncommitted changes in src/main.rs, src/terminal.rs, and guides/acp/progress.md
Commands: cargo fmt --all -- --check; cargo check --locked; cargo test --locked; if grep -nE '(^|[^e])println!' src/agent.rs; then exit 1; fi
Exit statuses: 0; 0; 0; 0
Tests run: 4
Tests passed: 4
Checker report path: none
```

Do not use prose explanations as completion evidence. Do not record credentials, complete model payloads, or unredacted sensitive paths.
