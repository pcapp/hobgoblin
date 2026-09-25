# Task 1 — Extract a reusable agent core

## End goal

Move conversation history outside the current `agent::run` call and make the agent core return its final answer instead of printing it. The existing `-p` command should still behave the same from a user's perspective.

This is production work in the existing application. Do not create an example executable.

## Why this comes first

The current `src/agent.rs::run` combines three responsibilities:

1. It creates conversation state.
2. It runs the model/tool loop.
3. It presents the final answer with `println!`.

That works for one prompt. It does not let a terminal conversation retain history, and an ACP frontend cannot allow the core to print arbitrary text to protocol stdout.

The useful boundary is not “CLI code versus ACP code.” It is **agent behavior versus frontend behavior**. Both future frontends should call the same turn operation.

## Rust concepts

### Ownership gives state its lifetime

A local `Vec<Value>` is dropped when its function returns. If conversation history must survive several turns, an owner with a longer lifetime must hold it. A session-like value can own the history, while one turn temporarily borrows it mutably.

The important relationship is:

```text
frontend owns conversation state
          ↓ mutable borrow
agent core performs one turn
          ↓ returned result
frontend presents the answer
```

You choose the concrete types and function names. A dedicated session struct is a natural option, but the task does not prescribe its final shape.

### Conversation is narrower than session

A conversation is the ordered message history visible to the model. A session is usually broader: it may later add an ID, working directory, configuration, lifecycle, or persistence. This task only needs conversation state, so a concrete wrapper is enough:

```rust
pub struct Conversation {
    messages: Vec<Value>,
}
```

`Vec` preserves message order, and `serde_json::Value` keeps the existing provider-formatted messages without redesigning the wire types. The frontend owns each `Conversation` value even if the type is defined in the agent module; one turn receives `&mut Conversation` and updates it. A session trait is unnecessary until there is genuinely interchangeable session behavior. If persistence is added later, the useful abstraction is more likely a session store.

Each new request sends the retained messages, which is how the model sees earlier turns. Context-limit handling is a separate later policy: this task does not need token counting, truncation, or summarization.

### Returning data separates behavior from presentation

Printing is an effect chosen for a human terminal. Returning an answer lets each caller decide what to do with it:

- The one-shot CLI prints it.
- The interactive CLI will print it and retain the session.
- ACP will eventually serialize it as a protocol update.

Diagnostic tracing is different from user-facing output and may continue to use stderr.

## Inspect before changing

Read these current paths:

- `src/main.rs`: where `agent::run` is called and where the provider is constructed.
- `src/agent.rs`: where `messages` is created, changed, and dropped; where the answer is printed.
- `src/tools.rs`: how tool results are added during one turn.

Before implementing, be able to answer:

1. Which messages must remain in history after a turn completes?
2. Which value should own that history?
3. Which layer should print an answer in one-shot mode?
4. What does the core return when the model completes without text?

## Implementation constraints

Your design must satisfy these behaviors:

- A caller creates conversation state before starting a turn.
- One turn mutates caller-owned history rather than replacing it with a new local history.
- Tool requests can still cause multiple model calls inside one turn.
- The final assistant message remains in history for the next turn.
- The core returns its user-facing result and does not print that result itself.
- `src/main.rs` creates a fresh conversation for `-p`, performs one turn, prints the returned answer, and exits.
- Existing provider and tool behavior should not be redesigned in this task.
- Do not add ACP types or an interactive input loop yet.

Keep provider-format history as its current JSON representation unless your own design needs a small wrapper. Replacing all provider DTOs is outside this task.

## Suggested implementation path

1. Establish the baseline with the commands below.
2. Introduce the caller-owned conversation state.
3. Change the core operation from “run the whole CLI” to “perform one turn.”
4. Return a meaningful result to the caller instead of printing it.
5. Update the existing `-p` path to create state, call the core, and present the result.
6. Review every early return in `agent.rs`; decide whether it represents success, an absent answer, or an error. Do not silently change unrelated error policy without understanding the effect.

The exact API is your design. If ownership or return types become difficult, stop and review that design rather than hiding the issue behind cloning or global state.

## Machine-verifiable acceptance

A build cannot prove the new ownership and output behavior. Add deterministic Rust tests using a scripted model/provider that returns fixed responses and records the requests it receives. The test double must not make network requests or read model credentials.

The automated suite must prove:

- Calling the turn operation twice with the same conversation includes the first user and assistant messages in the second provider request.
- Two separately created conversations do not share messages.
- A completed turn returns the scripted assistant text to its caller.
- The one-shot frontend invokes exactly one turn and sends the returned text to its output boundary.

Test names and module layout are your design, but all four behaviors must be represented by independently failing assertions.

Run:

```sh
cargo fmt --all -- --check
cargo check --locked
cargo test --locked
if grep -nE '(^|[^e])println!' src/agent.rs; then exit 1; fi
```

Task 1 is complete only when every command exits with status 0 and Cargo reports that the new behavioral tests ran and passed. No live model call or written explanation counts toward completion.

Record the commands and exit statuses in [progress.md](progress.md), then continue to [Task 2](02-interactive-cli.md).
