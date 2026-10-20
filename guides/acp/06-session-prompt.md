# Task 6 — Run a prompt turn over ACP

> **In progress.** The prompt wire types and focused parsing coverage are complete; dispatch integration is next.

## Working decisions

- A syntactically valid but unknown session ID returns ACP `-32002` (`Resource not found`).
- M6 accepts only text content blocks. `resource_link` and multimodal blocks are deliberately deferred, so this milestone does not yet satisfy the current ACP v1 baseline for prompt content.
- Multiple text blocks are joined in order with one newline.
- An empty prompt block array is invalid; an array containing an empty text block remains valid.

## End goal

`session/prompt` runs a real model turn against the session's conversation. The answer is reported as a `session/update` notification with `agent_message_chunk`, and the request is answered with `stopReason: "end_turn"`. While a turn runs, the reader keeps reading stdin.

**Milestone:** Hobgoblin is usable in Zed's agent panel after this task, configured as a custom agent server that launches `hobgoblin --acp`.

Reference: [ACP prompt turn](https://agentclientprotocol.com/protocol/v1/prompt-turn).

## Concepts

### One task per request

The reader dispatches each `session/prompt` into its own spawned task so it can keep reading. The handler task sends updates and the final response through the writer's channel.

### Making the core `Send`

`tokio::spawn` requires a `Send` future. ACP's transport error alias is already `Send + Sync`, but `Model::complete` is an `async fn` in a trait that does not promise a `Send` future, and the shared agent core still returns `Box<dyn std::error::Error>`. Expect compiler errors here and work through them yourself: they are the lesson. Look up `Send + Sync` error boxes and how to require `Send` from a trait's async method.

### Who holds the conversation during a turn

`agent::turn` needs `&mut Session` across many `.await`s. Holding a mutex guard that long without an explicit ownership decision is the Task 5 anti-pattern. Decide how a turn gets exclusive access to its session and what happens if a second prompt arrives for a busy session.

### Provider configuration happens late

ACP mode must start without a provider key. Configure the provider when a prompt needs it, and turn a missing key into a JSON-RPC error rather than a process exit.

## Required behavior

- Prompt text content blocks are joined into the user message; unsupported block types are rejected deliberately.
- The final answer is sent as at least one `agent_message_chunk` before the `session/prompt` response.
- Prompts for different sessions may run concurrently.
- Token-by-token streaming is not required; one chunk per answer is acceptable.

## Machine-verifiable acceptance (draft)

- A scripted `Model` test drives initialize → session/new → session/prompt over `tokio::io::duplex` and asserts update-before-response ordering.
- A test proves the reader handles another request while a prompt is still pending.
- Manual demo in Zed is recorded in the progress ledger but does not count as completion evidence.
