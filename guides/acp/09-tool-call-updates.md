# Task 9 — Report tool calls to the client

> **Draft.** Refine this from the Task 8 code before starting.

## End goal

Zed's agent panel shows each tool call as it happens: a `tool_call` update when the model requests a tool, `tool_call_update` with `in_progress` when it starts, and `completed` or `failed` with content when it finishes.

**Milestone:** record a demo of Hobgoblin running in Zed for the portfolio.

Reference: [ACP tool calls](https://agentclientprotocol.com/protocol/v1/tool-calls).

## Concepts

No new async concept. This task combines Tasks 6–8 and changes the core/frontend boundary.

### The core reports events, the frontend presents them

`agent::turn` currently returns only the final answer. To report tool progress, the core needs a way to emit events during the turn without knowing about ACP: for example, an event sink trait or a channel of core events. The ACP frontend maps those events to `session/update` notifications; the terminal frontend may ignore or print them.

## Before implementing, decide

1. What does a core event look like, and is it independent of ACP types?
2. How are tool `kind` and `title` derived for each tool?
3. Should file edits be reported as `diff` content?

## Required behavior

- Every tool call produces a `tool_call` update before execution and a final `tool_call_update`.
- Tool call IDs are unique within the session.
- The terminal frontend's behavior and tests are unchanged.

## Machine-verifiable acceptance (draft)

- A scripted-model test asserts the exact sequence of updates for one tool call followed by a final answer.
- Terminal frontend tests still pass.
