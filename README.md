# Hobgoblin

[![CI](https://github.com/pcapp/hobgoblin/actions/workflows/ci.yml/badge.svg)](https://github.com/pcapp/hobgoblin/actions/workflows/ci.yml)

Hobgoblin is a small coding-agent harness written in Rust. It runs as a terminal agent and as an agent inside [Zed](https://zed.dev/) over the [Agent Client Protocol (ACP)](https://agentclientprotocol.com/).

ACP is implemented directly with Tokio and Serde, not through an SDK, so the JSON-RPC framing, request dispatch, session registry and turn lifecycle are all in this repository. The project began as a solution to CodeCrafters' [Build Your Own Claude Code](https://codecrafters.io/challenges/claude-code) challenge and is also a hands-on exercise in Rust, async I/O, agent loops and protocol design.

## Current capabilities

- Runs in Zed's agent panel as a custom agent server, with multi-turn sessions
- One-shot prompts and interactive multi-turn conversations in the terminal
- Model-directed `Read`, `Write` and `Bash` tools
- OpenRouter access through an OpenAI-compatible client
- ACP over stdio: bounded newline-delimited JSON-RPC framing, separate reader and writer tasks, `initialize`, `session/new` with UUID v7 session IDs, and `session/prompt`
- Each prompt turn runs in its own task, so the agent keeps reading while the model works
- Deterministic tests with a scripted model, plus a black-box ACP test suite that drives the real binary

> **Project status:** usable in Zed and in the terminal. Prompt cancellation (`session/cancel`) is partly implemented. Tool calls are not yet reported to the client or gated by permission requests, and answers arrive as one message rather than streamed. See the [roadmap](guides/acp/milestones.md) and [progress ledger](guides/acp/progress.md).

## Requirements

- Rust 1.100 or newer
- An [OpenRouter](https://openrouter.ai/) API key. In ACP mode, `initialize` and `session/new` work without one; prompts need it.

## Setup

Create a `.env` file in the repository root:

```dotenv
OPENROUTER_API_KEY=your_api_key_here
```

`OPENROUTER_BASE_URL` is optional and defaults to `https://openrouter.ai/api/v1`.

Build the project:

```sh
cargo build --locked
```

## Usage

### In Zed

Build the binary, then add Hobgoblin as a custom agent server in Zed's `settings.json`:

```json
{
  "agent_servers": {
    "hobgoblin": {
      "type": "custom",
      "command": "/absolute/path/to/hobgoblin/target/debug/hobgoblin",
      "args": ["--acp"],
      "env": {
        "OPENROUTER_API_KEY": "your_api_key_here"
      }
    }
  }
}
```

Open a new thread with Hobgoblin from the agent panel. The `env` entry is only needed when Zed starts the agent from a directory where the `.env` file can't be found.

ACP mode reserves stdout for JSON-RPC messages and writes logs to stderr. You can also start it directly:

```sh
cargo run --locked -- --acp
```

### In the terminal

Start an interactive conversation:

```sh
cargo run --locked
```

Enter `/exit` or `/quit`, or send EOF, to end the session.

Run a single prompt:

```sh
cargo run --locked -- -p "Summarize the files in this repository"
```

Display all command-line options:

```sh
cargo run --locked -- --help
```

### Safety

The agent uses `anthropic/claude-haiku-4.5` and runs `Read`, `Write` and `Bash` tool calls on the local machine without asking first. Run it only in a directory where you are comfortable allowing model-directed file changes and shell commands.

## Architecture

Hobgoblin keeps one agent core and puts each frontend around it:

```text
                     agent core (src/agent.rs)
                conversation + model/tool loop
                  /            |            \
          one-shot -p     interactive      ACP over stdio
              (src/terminal.rs)            (src/acp.rs)
```

- `src/agent.rs` owns the model/tool loop. It returns the answer instead of printing it, so each frontend decides how to present it.
- `src/session.rs` holds a session's conversation, working directory and skills.
- `src/terminal.rs` provides the one-shot and interactive terminal frontends.
- `src/acp.rs` contains the ACP transport (a reader task and a single writer task joined by a channel), request dispatch and a per-connection session registry. Each `session/prompt` runs in its own task, with exclusive access to that session's conversation.
- `src/tools.rs` defines and runs the local tools; `src/skills.rs` loads skills from `.claude/skills`.
- `src/wire.rs` defines the provider-facing response types, kept separate from ACP's wire types.

A small `Model` trait sits between the core and the provider. Production uses OpenRouter; tests use a scripted model, so turns, history and ACP message ordering are tested without network access.

## Development

Run the full local pre-merge checks with:

```sh
./premerge.sh
```

This checks Rust formatting, lints and format-checks the Python ACP test suite, runs strict Clippy, and runs the Rust tests. CI runs the Rust checks:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

The black-box ACP suite drives the real binary over stdio and needs [uv](https://docs.astral.sh/uv/):

```sh
cargo build --locked
uv run --project guides/acp/acp_test_suite \
  guides/acp/acp_test_suite/test_handshake.py \
  --report /tmp/acp-test-suite.json \
  --executable target/debug/hobgoblin --acp
```

## Roadmap

Next: a structural cleanup of the ACP module, then finishing cancellation, then permission requests (`session/request_permission`) and tool-call updates, so Zed shows each tool call and its status. The course plan is in [`guides/acp`](guides/acp/README.md).
