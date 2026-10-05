# Task 7 — Cancel a prompt turn

> **Draft.** Refine this from the Task 6 code before starting.

## End goal

A `session/cancel` notification stops the session's running turn. The agent stops model and tool work as soon as possible, sends any final updates, then responds to the original `session/prompt` with `stopReason: "cancelled"`. A cancellation is never reported as an error.

Reference: the Cancellation section of [ACP prompt turn](https://agentclientprotocol.com/protocol/v1/prompt-turn).

## Concepts

### Cancellation by dropping

In Rust async, a future that is dropped stops at its last `.await`. `tokio::select!` races several futures and drops the losers; `JoinHandle::abort` cancels a spawned task. Both are ways to cancel a turn. A cancellation signal (`oneshot`, `watch`, or `tokio_util::sync::CancellationToken`) tells the turn that it should stop.

### Cancellation safety

Dropping a future at an arbitrary `.await` can leave state half-updated. Decide what the conversation history should contain after a cancelled turn, and make sure the session is usable for the next prompt.

### Blocking work cannot be cancelled by dropping

`src/tools.rs` runs `bash` with `std::process::Command`, which blocks a runtime worker thread and does not stop when the future is dropped. Look at `tokio::process::Command` and `kill_on_drop`.

## Before implementing, decide

1. Which mechanism signals cancellation, and who owns it?
2. Where does the turn check for cancellation: between model calls, during them, during tools?
3. What happens to conversation history on cancellation?

## Required behavior

- `session/cancel` for an idle or unknown session is ignored (it is a notification; it never receives a response).
- After cancellation the prompt response is `cancelled`, not an error, even if the model client returned an abort error.
- All updates for the turn are written before the prompt response.
- A new prompt on the same session works after cancellation.

## Machine-verifiable acceptance (draft)

- A test with a scripted model that blocks until released sends `session/cancel` mid-turn and asserts the `cancelled` stop reason.
- A test proves a long-running `bash` tool call is killed on cancellation.
