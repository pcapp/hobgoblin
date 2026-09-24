# Current learning roadmap

The immediate goal is to reach ACP initialization through the real application without building disposable examples. Later ACP behavior will be planned from the code that results.

| Task | Concrete result | Main concepts | Validation |
|---|---|---|---|
| [1 — Shared core](01-shared-core.md) | History outlives one call and the core returns an answer instead of printing it | Rust ownership, session lifetime, core/frontend boundary | Deterministic scripted-provider tests plus Cargo checks |
| [2 — Interactive CLI](02-interactive-cli.md) | Running without arguments holds a multi-turn conversation; `-p` remains one-shot | Outer conversation loop versus inner agent loop, EOF, frontend-owned state | In-memory frontend tests plus Cargo checks |
| [3 — ACP initialize](03-acp-initialize.md) | The real binary starts with `--acp` and completes ACP initialization without a model key | Tokio stdio, newline framing, JSON-RPC classification, correlation, stdout ownership | `check_handshake.py` against the real binary |

## Deliberately deferred

The current sequence does not yet implement:

- `session/new` or a session registry
- `session/prompt` or ACP updates
- Connecting ACP requests to the model loop
- ACP tool calls and permission requests
- Cancellation
- MCP
- Broad ACP compatibility claims

Those are expected future milestones, not requirements hidden inside the first three tasks.

## Loop for each task

1. Read the goal and inspect the named current code.
2. Choose a design that satisfies the constraints.
3. Implement one coherent change.
4. Add the deterministic acceptance coverage named by the task.
5. Run every listed validation command.
6. Record the commands, exit statuses, test count, and report path in [progress.md](progress.md).

Tests need not be written first. They must exist and pass before a task is marked complete.
