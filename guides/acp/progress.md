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
| [2 — Interactive CLI](02-interactive-cli.md) | complete | Multi-turn terminal conversation; `/exit`, `/quit`, and EOF | All required parser and interaction behaviors pass; all acceptance commands exited 0 on 2026-10-05 |
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

## Session checkpoint — 2026-10-01

### Current understanding

- Clap models a presence flag such as `--acp` with `bool`; `false` means absent and `true` means present.
- `run_interactive` owns one `Conversation` outside its read loop, so later turns include earlier user and assistant messages.
- Injected `BufRead` and `Write` boundaries make the entire interactive transcript deterministic, including prompts and the final prompt before EOF or an exit command.
- Terminal transcript assertions prove presentation behavior, while recorded model-request assertions prove turn count and shared conversation history.
- `/exit`, `/quit`, and EOF must be handled by the outer terminal loop before another call to `agent::turn`.

### Source checkpoint

- Task 2 implementation and nearly all required deterministic coverage are present.
- The branch is `master`, two commits ahead of `origin/master`, with an uncommitted change in `src/terminal.rs`.
- On 2026-10-01, formatting, Cargo check, all 13 tests, and `cargo run --locked -- --help` exited 0.
- Task 2 is not yet complete: `interactive_quit_command_does_not_start_another_turn` is named for `/quit` but its input currently contains `/exit`.

### Exact resume point

1. In `src/terminal.rs`, change the quit test input from `b"Hello.\n/exit\n"` to `b"Hello.\n/quit\n"`.
2. Run the focused quit test, then all four Task 2 acceptance commands.
3. If every command exits 0, mark Task 2 complete and add its final validation record below.
4. Begin Task 3 by inspecting `src/main.rs`, `Cargo.toml`, `src/wire.rs`, and `guides/acp/check_handshake.py`; the first design decision is where ACP framing and dispatch should live.

Rust principles practiced in Task 2: model mutually exclusive CLI options with Clap, keep conversation state outside the outer loop, distinguish EOF from an empty line, inject synchronous I/O traits for deterministic frontend tests, and assert internal requests when output alone cannot prove retained state.

## Session checkpoint — 2026-10-05

### Current understanding

- Task 2's acceptance suite covers mode selection, parser rejection cases, shared interactive history, both exit commands, EOF, and one-shot behavior without a terminal, network, or API key.
- The `/quit` regression test now feeds `/quit`, so its passing result is direct evidence for that required behavior.
- All four acceptance commands must pass in the same verified tree before the milestone can be closed.

### Source checkpoint

- Task 2 is complete.
- The branch is `master`, three commits ahead of `origin/master`; uncommitted guide changes are present.
- On 2026-10-05, the focused `/quit` test and all four Task 2 acceptance commands exited 0; the full suite passed all 13 tests.
- No Task 2 work remains unverified.

### Exact resume point

1. Begin Task 3 by reading `03-acp-initialize.md`.
2. Inspect `src/main.rs`, `Cargo.toml`, `src/wire.rs`, and `guides/acp/check_handshake.py` before choosing where ACP framing and dispatch should live.
3. Verify the current behavior of `--acp` before making the first Task 3 change.

## Session checkpoint — 2026-10-05 (Task 3 started)

### Current understanding

- ACP initialization is a protocol concern and does not need a model provider or API key.
- `--acp` must be routed before dotenv loading and OpenRouter client construction.
- `BufRead::read_line` returns `0` when EOF occurs before any bytes are read, allowing clean EOF to end ACP mode successfully.
- Missing test coverage should be described by the behavior it must prove; the learner writes the test unless they ask for implementation help.

### Source checkpoint

- Task 3 is in progress.
- `src/main.rs` routes `--acp` before provider configuration.
- `src/acp.rs` treats empty stdin as clean EOF, with a focused regression test.
- Formatting, Cargo check, all 16 tests, a build, and a credential-free subprocess probe passed before this checkpoint was committed.

### Exact resume point

1. Continue replacing the synchronous ACP stdio scaffold with Tokio I/O.
2. Identify the Tokio features required for async stdin/stdout and buffered async reads.
3. Decide the async reader boundary needed by the next incremental change, then implement and verify only that change.
4. Bounded framing, response writing, dispatch, and full checker acceptance remain unverified.

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

```text
Date: 2026-10-05
Task: 2 — Interactive CLI
Commit or tree state: master, three commits ahead of origin/master; guides/acp/progress.md was modified when validation began
Commands: cargo fmt --all -- --check; cargo check --locked; cargo test --locked; cargo run --locked -- --help
Exit statuses: 0; 0; 0; 0
Tests run: 13
Tests passed: 13
Checker report path: none
```

Do not use prose explanations as completion evidence. Do not record credentials, complete model payloads, or unredacted sensitive paths.
