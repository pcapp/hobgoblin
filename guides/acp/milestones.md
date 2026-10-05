# Current learning roadmap

The immediate goal is to reach ACP initialization through the real application without building disposable examples. Tasks 4–9 are drafts: each adds one async concept driven by the ACP feature that needs it, and each is refined from the previous task's code before it starts.

| Task | Concrete result | Main concepts | Validation |
|---|---|---|---|
| [1 — Shared core](01-shared-core.md) | History outlives one call and the core returns an answer instead of printing it | Rust ownership, session lifetime, core/frontend boundary | Deterministic scripted-provider tests plus Cargo checks |
| [2 — Interactive CLI](02-interactive-cli.md) | Running without arguments holds a multi-turn conversation; `-p` remains one-shot | Outer conversation loop versus inner agent loop, EOF, frontend-owned state | In-memory frontend tests plus Cargo checks |
| [3 — ACP initialize](03-acp-initialize.md) | The real binary starts with `--acp` and completes ACP initialization without a model key | Tokio stdio, newline framing, JSON-RPC classification, correlation, stdout ownership | `check_handshake.py` against the real binary |
| [4 — Reader and writer tasks](04-reader-writer-tasks.md) (draft) | Same ACP behavior; stdin reader and stdout writer run as separate tasks | `tokio::spawn`, `mpsc`, `'static + Send`, shutdown by channel closure, `tokio::io::duplex` tests | Handshake checker unchanged plus duplex tests |
| [5 — Session creation](05-session-new.md) (draft) | `session/new` returns unique IDs backed by a session registry | `Arc<Mutex<…>>` versus an owning task; never hold a lock across `.await` | Extended checker plus unit tests |
| [6 — Prompt turn](06-session-prompt.md) (draft) | `session/prompt` runs the model and reports `agent_message_chunk`; **usable in Zed** | One task per request, `Send` futures and errors, exclusive access to a conversation | Scripted-model duplex tests |
| [7 — Cancellation](07-cancellation.md) (draft) | `session/cancel` ends a turn with `stopReason: "cancelled"` | `select!`, cancellation by dropping, cancellation safety, `tokio::process` | Mid-turn cancellation tests |
| [8 — Permissions](08-permissions.md) (draft) | Tools ask the client via `session/request_permission` | Agent-to-client requests, pending-request table, `oneshot`, `timeout` | Deadlock-proof duplex tests |
| [9 — Tool call updates](09-tool-call-updates.md) (draft) | Zed shows tool calls and their status; **portfolio demo** | Core events versus frontend presentation | Update-sequence tests |

## Deliberately deferred

The current sequence does not yet plan:

- Token-by-token streaming from the provider
- `session/load` and persistence
- Client file-system and terminal methods (`fs/*`, `terminal/*`)
- MCP
- Broad ACP compatibility claims

## Loop for each task

1. Read the goal and inspect the named current code.
2. Choose a design that satisfies the constraints.
3. Implement one coherent change.
4. Add the deterministic acceptance coverage named by the task.
5. Run every listed validation command.
6. Record the commands, exit statuses, test count, and report path in [progress.md](progress.md).

Tests need not be written first. They must exist and pass before a task is marked complete.
