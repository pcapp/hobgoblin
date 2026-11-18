# Task 8 — Report turn progress to the client

> **Draft.** Refine this from the Task 7 code before starting.

## End goal

Zed's agent panel updates while a turn runs, not only when it ends. Assistant text appears as the model produces each reply, and each tool call appears as it happens: a `tool_call` update when the model requests a tool, `tool_call_update` with `in_progress` when it starts, and `completed` or `failed` with content when it finishes.

This is turn-level progress: one update per model reply or tool step. Token-by-token streaming from the provider stays deferred.

**Milestone:** record a demo of Hobgoblin running in Zed for the portfolio.

Reference: [ACP prompt turn](https://agentclientprotocol.com/protocol/v1/prompt-turn) and [ACP tool calls](https://agentclientprotocol.com/protocol/v1/tool-calls).

## Concepts

No new async primitive. This task changes the core/frontend boundary and makes message ordering something you must design for.

### The core reports events, the frontend presents them

`agent::turn` currently returns only the final answer, so text the model sends alongside tool calls never reaches the client, and tool calls are invisible. To report progress, the core needs a way to emit events during the turn without knowing about ACP: for example, an event sink trait or a channel of core events. The ACP frontend maps those events to `session/update` notifications; the terminal frontend may ignore or print them.

### Ordering across tasks and channels

Task 7 requires every update for a turn to be written before the prompt response. Today that holds because one task sends both through the writer channel. If events travel a different path from the response (a second channel, a forwarding task), the response can overtake the last updates. Check the ordering on both the normal and the cancelled path.

## Before implementing, decide

1. What does a core event look like, and is it independent of ACP types?
2. How does the core emit events: a trait, a channel, or a callback? Who owns the receiving end?
3. How do you keep the final answer from being reported twice, once as an event and once after the turn?
4. How are tool `kind` and `title` derived for each tool?
5. Should file edits be reported as `diff` content?

## Required behavior

- Assistant text from every model reply in the turn is reported as `agent_message_chunk`, including text that accompanies tool calls.
- Every tool call produces a `tool_call` update before execution and a final `tool_call_update`.
- Tool call IDs are unique within the session.
- All updates are written before the prompt response, including on cancellation.
- The terminal frontend's behavior and tests are unchanged.

## Machine-verifiable acceptance (draft)

- A scripted-model test asserts the exact sequence of updates for one tool call followed by a final answer.
- A cancellation test asserts that updates already emitted precede the `cancelled` response.
- Terminal frontend tests still pass.
