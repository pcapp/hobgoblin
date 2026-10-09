# Task 4 — Split the transport into reader and writer tasks

> **Draft.** Refine this from the Task 3 code before starting.

## End goal

ACP behavior is unchanged, but the transport is restructured: one task reads frames from stdin, one task exclusively owns stdout, and they communicate through a channel. The Task 3 handshake checker still passes unchanged.

This is a pure refactor. Its purpose is to learn spawned tasks and channels while an existing checker proves nothing broke.

## Concepts

### Tasks are independently scheduled futures

`tokio::spawn` hands a future to the runtime, which polls it alongside other tasks. A spawned future must be `'static` (it owns everything it uses; it borrows nothing from the spawner) and `Send` (it may move between worker threads). These bounds explain most of the compiler errors you will meet in this task.

`spawn` returns a `JoinHandle`. Awaiting it waits for the task to finish and returns its output.

### Channels transfer ownership between tasks

`tokio::sync::mpsc` gives many senders and one receiver. Sending moves the value into the channel; no data is shared. The writer task owns the receiver and is therefore the only code that can write to stdout. Any part of the program that needs to emit a message receives a cloned sender.

### Shutdown through channel closure

When every `Sender` is dropped, `recv()` returns `None`. This makes a natural shutdown sequence: the reader hits EOF and returns, its sender is dropped, the writer drains remaining messages, flushes, and exits, and the main task awaits both handles.

### In-memory full-duplex tests

`tokio::io::duplex` creates a connected pair of in-memory streams. A test can drive the transport as a client would without spawning a process. This only works if the transport accepts generic async readers and writers rather than calling `stdin()` and `stdout()` directly.

## Working design

- The channel carries complete `serde_json::Value` messages.
- Dispatch constructs complete JSON-RPC messages and returns `Result<Option<Value>, _>`.
- `Some(value)` means the writer should emit a message; `None` means valid handling with no response.
- The writer owns compact serialization, newline framing, writing, flushing, and the output stream.
- During Task 4's sequential handling, the reader task owns the only sender. Reaching EOF drops that sender, allowing the writer to drain the queue and exit.

## Decide when needed

1. Which Tokio feature enables `tokio::sync::mpsc`?
2. How does a writer-task error, such as closed stdout, reach the coordinating task?

## Implementation order

1. Refactor `dispatch` to return `Result<Option<Value>, _>` while `run_acp` remains sequential.
2. Run the existing tests and handshake checker to prove the new boundary preserves behavior.
3. Add the channel and dedicated writer loop without spawning tasks yet.
4. Move the reader and writer loops into spawned tasks.
5. Add duplex coverage for a response received before client EOF and for draining queued responses after EOF.

## Required behavior

- The reader never writes to stdout. The writer is the only stdout owner.
- Each outgoing message is written in full and flushed before the next is written.
- Clean EOF on stdin produces a clean exit after all queued responses are flushed.
- The handshake checker passes without modification.

## Implementation boundaries

- Do not add new ACP methods.
- Handling may stay sequential inside the reader task; concurrency between handlers arrives in Task 6.

## Machine-verifiable acceptance (draft)

- Task 3's local checks and `check_handshake.py` all exit 0.
- At least one `tokio::io::duplex` test sends `initialize` and reads the correlated response without closing the client side first.
- At least one test proves that queued responses are flushed when input reaches EOF.
