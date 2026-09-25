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
| [1 — Shared core](01-shared-core.md) | in progress | Caller-owned history; returned answer; `-p` preserved | Design explored; implementation and acceptance tests not complete |
| [2 — Interactive CLI](02-interactive-cli.md) | not started | Multi-turn terminal conversation; `/exit`, `/quit`, and EOF | — |
| [3 — ACP initialize](03-acp-initialize.md) | not started | Real binary completes ACP initialization through Tokio stdio | — |

Use `not started`, `in progress`, `complete`, or `blocked`. Mark a task complete only when every listed command exits with status 0 and its required automated cases are present in the test/checker output.

## Session checkpoint — 2026-09-24

### Current understanding

- `agent::run` currently retains messages only during one model/tool loop; its local vector is dropped when the call returns.
- A conversation is the ordered model-visible message history. A session may later include identity, configuration, lifecycle, and persistence.
- A concrete `Conversation { messages: Vec<Value> }` is sufficient now. `Vec` preserves order, while `Value` preserves the existing provider JSON representation.
- The terminal frontend should own each conversation value. The agent core should perform one turn by borrowing it as `&mut Conversation`.
- The terminal owns stdin, prompts, `/quit`, and answer printing. The agent core should neither read stdin nor print user-facing output.
- Retained messages let the next request see prior answers. Context-window management for very long histories is a separate later concern.

### Source checkpoint

- `src/terminal.rs` exists with one-shot and interactive entry points and an input-loop scaffold.
- `Conversation` is currently defined in `terminal.rs`, but it is not passed to the agent.
- `src/agent.rs::run` still creates its own `Vec<Value>` and prints the final answer.
- The interactive loop is ahead of Task 1; finish the shared turn boundary before extending it.

### Exact resume point

1. Define the conversation type at the shared core boundary while keeping its messages private.
2. Change `agent::run` into a one-turn operation that accepts `&mut Conversation`.
3. Append the user, assistant, and tool messages to that caller-owned history.
4. Return the final assistant text instead of printing it.
5. Make one-shot mode create one conversation and print the returned answer.
6. Add the deterministic acceptance tests required by Task 1 before returning to the interactive loop.

No Task 1 completion evidence has been recorded. The current implementation remains unverified against the task's behavioral acceptance criteria.

## Validation record

Add one structured entry after validation:

```text
Date:
Task:
Commit or tree state:
Commands:
Exit statuses:
Tests run:
Tests passed:
Checker report path:
```

Do not use prose explanations as completion evidence. Do not record credentials, complete model payloads, or unredacted sensitive paths.
