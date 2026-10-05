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

## Before implementing, decide

1. What type travels through the channel: serialized lines, `serde_json::Value`, or a typed outgoing-message enum?
2. Who holds senders, and how does the last one get dropped at EOF?
3. Which Tokio features does `tokio::sync` and `tokio::io::duplex` require?
4. How does an error in the writer task (for example, a closed stdout) reach the main task?

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
