# Lab 00 — One agent, two frontends

## Goal

Design an agent that supports both an interactive terminal and an editor connection without duplicating its model/tool loop.

You will identify which parts of the current program belong to the agent, which belong to its frontend, and who should own conversation state. This architecture will guide the implementation in the following labs.

**Deliverable:** an annotated architecture diagram, a responsibility map tied to the current source, and a plan for the first two refactors. No application changes are required in this lesson.

## Before you begin

Open the Rust repository:

```sh
cd /Users/peter/repos/codecrafters-claude-code-rust
cargo check --locked
```

`cd` selects the project directory. `cargo check` type-checks the existing code. `--locked` prevents Cargo from editing `Cargo.lock`; it can still download locked dependencies that are missing locally.

Open these files in your editor:

- `src/main.rs`: arguments, logging, provider setup, and entry point.
- `src/agent.rs`: conversation history and the model/tool loop.
- `src/tools.rs`: tool definitions and local execution.
- `src/wire.rs`: model-provider response types.

Locate the named functions as you work; line numbers below are starting points and can move after edits. This lesson needs no model request or API key.

## 1. Separate the agent from its frontend

A **frontend** translates between an external interface and the agent's operations. The terminal reads user input and displays text. The ACP frontend reads protocol messages and sends protocol responses.

The agent core performs a turn: it maintains the conversation, calls the model, executes permitted tools, and decides when to stop.

```text
                     main: select mode
                      /             \
           terminal frontend       ACP frontend
           ├─ one-shot -p          ├─ JSON-RPC transport
           └─ interactive loop     └─ session/request routing
                      \             /
                       shared agent core
                       ├─ conversation state
                       ├─ model/tool loop
                       ├─ events and outcomes
                       └─ provider and tool interfaces
```

Keep one executable. Choose its frontend once at startup rather than checking the mode throughout the agent loop.

The terminal frontend can support two behaviors:

- **One-shot:** accept the existing `-p` prompt, perform one turn, and exit.
- **Interactive:** read a prompt, perform a turn, and repeat within the same conversation.

The ACP frontend exposes operations such as creating a session, submitting a prompt, and cancelling work. It does not implement a second model/tool loop.

**Exercise:** trace the existing path from `main` to `agent::run`. Label each operation as frontend setup, agent behavior, provider communication, or tool execution.

**Checkpoint:** explain why placing `if acp_mode` around the answer's `println!` would not be a complete separation.

## 2. Identify the two different loops

The current loop in `src/agent.rs::run` is the **inner agent loop**:

```text
Append the user's prompt
Repeat within the turn's budget:
    Ask the model for its next response
    If it requests tools:
        Execute permitted tools
        Append their results
    Otherwise:
        Finish the turn
```

A single user prompt can require several model requests and tool calls. That entire sequence is one **turn**.

Interactive mode adds an **outer conversation loop**:

```text
Create a session
Repeat until the user exits:
    Read the next user prompt
    Run one agent turn using that session
    Present its events and final outcome
```

ACP supplies the outer interaction through messages instead:

| Operation | Frontend responsibility |
|---|---|
| `session/new` | Create and store a session; return its identifier |
| `session/prompt` | Find that session and start one turn |
| `session/cancel` | Signal the active turn to stop |

Both frontends call the same turn engine. Neither needs to reproduce the engine's model/tool loop.

**Exercise:** draw a conversation containing two user prompts. Within the first turn, draw a model response requesting a tool, the tool result, and another model response. Label the session, each turn, and each model request.

**Checkpoint:** distinguish “read the next user prompt” from “ask the model what to do next.” Which loop owns each?

## 3. Give conversation history a longer lifetime

Inspect `src/agent.rs:7–15`. The current `run` function creates a local `messages` vector. When the function returns, that vector is dropped. Calling `run` again starts a new conversation.

For a conversation spanning several turns, move history into a **Session** owned outside the turn function:

| Mode | Session ownership and lifetime |
|---|---|
| One-shot terminal | Frontend creates one session, runs one turn, then drops it |
| Interactive terminal | Frontend retains one session across prompts until exit/reset |
| ACP | Frontend retains sessions in a registry keyed by session ID |

A session initially needs conversation history and a working directory. Keep provider-format history as the existing JSON values for the first refactor; redesigning every message type is a separate task.

A turn receives exclusive access to one session's mutable history. Its prompt and validated model/tool messages become part of that history. Another turn in the same session sees those messages.

For the initial design, permit one active turn per session. Different ACP sessions can run independently. A background turn task must own the state it uses, or access it through a deliberate per-session ownership design; it must not borrow temporary input buffers or hold a global session-map lock while awaiting a model response.

**State across turns does not require persistence across process restarts.** Start with memory. Saving and loading conversations is a later feature.

**Exercise:** draw who owns the session before, during, and after a turn for interactive mode and ACP mode. Mark the point at which a second prompt for a busy session would be rejected.

**Checkpoint:** describe what stays alive when a turn returns, and what is discarded when the process exits.

## 4. Report events; let the frontend present them

Find `println!` in `src/agent.rs:59–64`. The core currently chooses how the answer appears. That ties agent behavior to a terminal.

Instead, the core reports domain events and returns a final outcome:

| Core event | Terminal presentation | ACP presentation |
|---|---|---|
| `AssistantText(text)` | Display text | Send a message update |
| `ToolStarted(...)` | Display tool activity | Send a tool-call update |
| `ToolFinished(...)` | Display result/status | Send a tool-call update |

These names are a proposed application vocabulary, not required protocol type names.

An **event** says what happened while a turn ran. An **outcome** says why it stopped: normal completion, cancellation, or budget exhaustion. An **error** says the operation failed. Do not collapse all three into printing a message and returning `Ok(())`.

For the first refactor, returning one answer value is enough. Introduce an event enum and a delivery boundary when incremental text or tool progress is needed. A bounded Tokio channel is one possible implementation; a trait or callback is another. You do not need a generic plugin system or dynamic dispatch to establish the separation.

The core must not:

- Read terminal input.
- Print user-facing answers or errors directly.
- Construct ACP envelopes or allocate protocol request IDs.
- Know which frontend will display its events.

Diagnostic tracing can remain separate from user-facing output. In ACP mode, stdout belongs exclusively to the protocol writer; diagnostics go to stderr.

**Exercise:** inspect the exits in `src/agent.rs:39–50,59–64,81–86`. Classify each as a normal outcome or an error, and identify which ones currently return success despite a failure.

**Checkpoint:** explain why sending answer text and finishing a turn are separate operations.

## 5. Separate tool execution from presentation

Inspect `src/agent.rs:66–77` and `src/tools.rs::execute_tool_call`. Tool calls currently lead directly to local filesystem or process operations.

Changing the answer's output destination does not change those side effects. Add a distinct tool-execution boundary:

| Component | Responsibility |
|---|---|
| Agent core | Interpret a model's requested tool call and incorporate its result into the conversation |
| Tool layer | Validate arguments, enforce execution policy, obtain required approval, and execute the chosen operation |
| Terminal integration | Provide terminal approval and the local execution route |
| ACP integration | Request approval and editor filesystem/terminal operations where appropriate |

A model request is not authorization. Showing a tool card is not approval, and a frontend capability is not permission to use it.

The core should await a tool result through this boundary without knowing whether it came from a local operation, an editor request, or a scripted test double. Terminal prompting and ACP permission messages stay outside the core.

**Exercise:** trace a proposed file overwrite in each frontend. Mark validation, permission, actual execution, and result reporting as separate steps. Draw the denial path so it ends before execution.

**Checkpoint:** identify what can be shared between modes and what must be supplied by the frontend integration.

## 6. Keep startup and modules simple

Use this as a target responsibility map, not a request to create empty files immediately:

```text
src/
  main.rs       Arguments, logging, startup, mode selection
  cli.rs        One-shot and interactive terminal frontend
  acp.rs        Protocol transport and session/request routing
  agent.rs      Shared turn engine
  session.rs    Conversation history and working directory
  tools.rs      Tool definitions and execution boundary
  wire.rs       Existing model-provider message types
```

Keep ACP wire types separate from the provider types in `wire.rs`. The editor-to-agent protocol and the agent-to-model API serve different purposes.

At startup, select the frontend before requiring model credentials. The ACP connection must be able to initialize before a model call is needed. Interactive/one-shot model use can report missing configuration when that operation is requested.

In ACP mode, one component reads stdin and one serialized writer owns stdout. Slow turn work runs independently so the protocol reader can still receive cancellation and replies to requests sent to the client. The synchronous handshake in Lab 01 is a small transport exercise, not the final concurrency architecture.

For the first interactive terminal version, read the next prompt after the current turn finishes. Concurrent terminal input and Ctrl-C cancellation require their own coordination; do not add another reader competing for the same stdin stream.

**Exercise:** annotate the module tree with allowed dependencies. The frontend may depend on the core; the core should not depend on `cli` or `acp`.

## 7. Plan the first two refactors

Before connecting the existing agent to either a terminal conversation loop or ACP prompts, make these changes in small, separately verified steps:

1. **Caller-owned history.** Move history's lifetime outside `agent::run`. The one-shot caller creates a fresh session, preserving its existing behavior. A repeated caller can retain that session.
2. **Presentation-free answers.** Replace direct answer printing in the core with a returned value or event. The terminal frontend becomes responsible for displaying it.

Write down each proposed change, its source target, and a test that would fail before the change. Use this test plan:

| Test | Evidence it should produce |
|---|---|
| Same session, two turns | A scripted provider captures a second request containing the first turn's user and assistant messages in order |
| Separate sessions | The second session's provider request contains none of the first session's history |
| Core output boundary | An in-memory answer/event sink receives the text; a test driver captures no direct core stdout/stderr output with diagnostic tracing disabled |
| One-shot compatibility | A scripted provider produces the same terminal answer and exit behavior through the `-p` path |

A **scripted provider** returns known responses and records requests. It tests your ownership and control flow without model costs or unpredictable answers. The provider interface should be no broader than needed for the real provider and this test implementation.

Today, design these tests rather than implementing the whole refactor. Lab 01 builds an independent handshake, Lab 02 builds the frontend transport with echo behavior, and Lab 03 implements this core refactor before connecting model turns. The interactive terminal loop can then reuse the same engine and retained session.

## Completion checklist

- [ ] Draw one core shared by terminal and ACP frontends.
- [ ] Distinguish a session, a turn, and a model request.
- [ ] Identify where the current program loses history and prints answers.
- [ ] Explain session ownership in one-shot, interactive, and ACP modes.
- [ ] Separate events, final outcomes, and errors.
- [ ] Draw a tool permission path that cannot execute after denial.
- [ ] Describe the first two refactors and their deterministic tests.

Bring your diagram and source-linked notes to review. Be ready to answer: **what changes when the frontend changes, and what must remain the same?**

Use the [source-inspection worksheet](00-inspect.md) for the detailed ownership and wire-direction questions. Then continue to [Lab 01 — Build an ACP handshake](01-handshake.md).

[Course](README.md) · [Milestones](milestones.md)
