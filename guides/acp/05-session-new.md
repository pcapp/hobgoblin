# Task 5 — Create sessions

> **Draft.** Refine this from the Task 4 code before starting.

## End goal

The agent handles `session/new`, returns a unique `sessionId`, and keeps a registry of live sessions that later requests can look up. Each session owns its own `Conversation`.

Reference: [ACP session setup](https://agentclientprotocol.com/protocol/v1/session-setup).

## Concepts

### Shared state versus an owning task

Two common shapes for state that several tasks need:

- **Shared:** `Arc<Mutex<HashMap<SessionId, Session>>>`. `Arc` gives shared ownership across tasks; `Mutex` gives exclusive access while the guard is alive.
- **Owned:** one task owns the map and others send it requests over a channel (the same idea as Task 4's writer).

Either is acceptable here. Choose one and be able to say why.

### Never hold a lock across `.await`

A `std::sync::MutexGuard` held across an `.await` blocks other tasks for as long as the awaited work takes, and the guard is not `Send`, so the compiler rejects spawning such a future. Keep critical sections short and synchronous: lock, copy or take what you need, unlock, then await. `tokio::sync::Mutex` can be held across `.await`, but it is slower and usually a sign the design can be simpler.

This is one of the most common async bugs to spot in an agent's diff.

## Before implementing, decide

1. Which representation does `SessionId` have, and how is uniqueness guaranteed?
2. Which session params (`cwd`, `mcpServers`) are validated, stored, or ignored for now?
3. Shared state or owning task?
4. What does a request for an unknown session ID return?

## Required behavior

- `session/new` before `initialize` is rejected deliberately.
- `session/new` returns a unique `sessionId` and creates an empty conversation.
- `mcpServers` is accepted but not acted on; advertise no MCP capabilities.
- No model call and no provider key are needed.

## Machine-verifiable acceptance (draft)

- An extended subprocess checker creates two sessions and verifies distinct IDs.
- Unit tests cover unknown-session lookups and session creation before initialization.
