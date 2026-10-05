# Task 8 — Ask the client for permission

> **Draft.** Refine this from the Task 7 code before starting.

## End goal

Before running a tool that writes files or executes commands, the agent sends a `session/request_permission` request to the client and waits for the user's decision. Allowed tools run; rejected tools are reported back to the model as rejected.

Reference: Requesting Permission in [ACP tool calls](https://agentclientprotocol.com/protocol/v1/tool-calls).

## Concepts

### The agent is also a JSON-RPC client

Until now, only Zed sent requests. Now the agent sends a request and the **response arrives on stdin**, read by the reader task. If the reader is waiting for the turn while the turn waits for the reply, both wait forever.

### The pending-request table

The usual fix is a map from outgoing request ID to a `tokio::sync::oneshot::Sender`. The turn inserts an entry, sends the request through the writer, and awaits the `oneshot::Receiver`. When the reader classifies an incoming message as a response, it removes the matching entry and sends the response into it. Responses with unknown IDs are logged to stderr and dropped.

### Timeouts make hangs testable

`tokio::time::timeout` turns a hang into an error. Use it in tests so a deadlock fails quickly instead of stalling the test run.

## Before implementing, decide

1. How are outgoing request IDs allocated so they never collide?
2. Where does the pending table live, and who can reach it?
3. What happens to pending permission requests when a turn is cancelled? (The client answers them with the `cancelled` outcome.)
4. Which tools require permission?

## Required behavior

- Permission requests carry the session ID, the tool call ID, and at least allow-once and reject-once options.
- `selected` with an allow option runs the tool; a reject option or `cancelled` outcome does not.
- The reader routes responses without waiting for any turn.

## Machine-verifiable acceptance (draft)

- A duplex test where the client's permission response arrives after the prompt request completes within a timeout.
- Tests for allow, reject, and cancelled outcomes.
