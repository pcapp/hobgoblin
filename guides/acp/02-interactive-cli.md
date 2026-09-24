# Task 2 — Add an interactive multi-turn terminal

## End goal

Running the application without arguments starts an interactive conversation that reuses one conversation history across prompts.

The command-line modes should be:

| Invocation | Behavior |
|---|---|
| No arguments | Start an interactive multi-turn conversation |
| `-p "prompt"` | Perform one turn and exit |
| `--acp` | Reserved for Task 3; it does not need to work yet |

The interactive conversation exits on `/exit`, `/quit`, or terminal EOF. In a normal terminal, Ctrl-D reports EOF; it does not send a `SIGEOF` signal.

## Concepts

### A conversation loop is not the agent loop

The core already has an inner loop: one user prompt may lead to several model requests and tool calls before the assistant finishes its answer. That entire process is one **turn**.

Interactive mode adds an outer loop:

```text
create one conversation
repeat:
    read one user prompt
    perform one agent turn
    display the returned answer
```

The terminal frontend owns this outer loop. The agent core continues to own the inner model/tool loop.

### Reuse state, not control flow

One-shot and interactive modes should share the same turn operation. They differ only in how long the frontend keeps the conversation state and how many prompts it reads.

Avoid copying the model/tool loop into a CLI module. Duplication here would make the future ACP frontend a third implementation of agent behavior.

### EOF is an input state

An input API must distinguish:

- A line containing text.
- An empty line.
- End-of-input.
- An input error.

EOF should finish the interactive session cleanly. Decide deliberately what an empty or whitespace-only line means; do not accidentally submit it as a paid model request.

## Inspect before changing

Review the API you produced in Task 1 and the current argument declaration in `src/main.rs`.

Before implementing, decide:

1. Which frontend function owns the interactive conversation value?
2. How will argument parsing distinguish no arguments from `-p`?
3. Will `/exit` and `/quit` be recognized exactly or after trimming whitespace?
4. How will input and provider errors affect the next iteration or process exit?
5. What prompt text, if any, belongs on stdout?

The final question matters because Task 3 will reserve stdout for protocol frames in ACP mode. Terminal prompts are fine in interactive mode, but output ownership must be mode-specific.

## Implementation constraints

- Running with no arguments enters interactive mode.
- One conversation value is created before the read loop and reused for every turn.
- Each ordinary input line invokes the same core turn operation used by `-p`.
- `/exit`, `/quit`, and EOF exit successfully without making another model request.
- `-p` remains one-shot and does not enter the input loop.
- Empty input is handled deliberately.
- The inner model/tool loop remains in the core.
- Do not implement ACP framing in this task.

You may keep mode selection in `main.rs` or introduce a small CLI module if it has a concrete responsibility. Do not create empty architecture modules in anticipation of future work.

## Machine-verifiable acceptance

Make the interactive frontend testable with in-memory input/output and a scripted turn implementation. The tests may be unit or integration tests, but they must not use a terminal, network, API key, or live model.

The automated suite must prove:

- No arguments select interactive mode.
- `-p hello` selects one-shot mode.
- A missing value after `-p` is rejected.
- `--acp` and `-p` are rejected when supplied together.
- Two ordinary input lines invoke two turns against the same conversation state.
- `/exit` and `/quit` exit without invoking another turn.
- EOF exits successfully without invoking another turn.
- The one-shot path invokes exactly one turn and does not enter the interactive read loop.

Run:

```sh
cargo fmt --all -- --check
cargo check --locked
cargo test --locked
cargo run --locked -- --help
```

Task 2 is complete only when every command exits with status 0 and Cargo reports that all listed parser and interaction behaviors ran and passed. No live conversation or written explanation counts toward completion.

Record the commands and exit statuses in [progress.md](progress.md), then continue to [Task 3](03-acp-initialize.md).
