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
| [1 — Shared core](01-shared-core.md) | not started | Caller-owned history; returned answer; `-p` preserved | — |
| [2 — Interactive CLI](02-interactive-cli.md) | not started | Multi-turn terminal conversation; `/exit`, `/quit`, and EOF | — |
| [3 — ACP initialize](03-acp-initialize.md) | not started | Real binary completes ACP initialization through Tokio stdio | — |

Use `not started`, `in progress`, `complete`, or `blocked`. Mark a task complete only when every listed command exits with status 0 and its required automated cases are present in the test/checker output.

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
