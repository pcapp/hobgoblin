# Hobgoblin

[![CI](https://github.com/pcapp/hobgoblin/actions/workflows/ci.yml/badge.svg)](https://github.com/pcapp/hobgoblin/actions/workflows/ci.yml)

Hobgoblin is a small coding-agent harness written in Rust. It began as a solution to CodeCrafters' [Build Your Own Claude Code](https://codecrafters.io/challenges/claude-code) challenge and is evolving into an agent that can run inside editors such as Zed over the [Agent Client Protocol (ACP)](https://agentclientprotocol.com/).

The project is also a hands-on exercise in Rust, asynchronous I/O, agent loops, tool calling, and protocol design. ACP is implemented directly with Tokio and Serde rather than through an SDK.

## Current capabilities

- One-shot prompts from the command line
- Interactive, multi-turn terminal conversations
- OpenRouter access through an OpenAI-compatible client
- Model-directed tools for reading files, writing files, and running shell commands
- Deterministic tests using a scripted model implementation
- Early ACP transport work, including asynchronous bounded frame reading

> **Project status:** terminal modes are usable. The ACP frontend is under active development and does not yet complete the initialization handshake or run prompt sessions. See the [roadmap](guides/acp/milestones.md) and [progress ledger](guides/acp/progress.md) for the exact status.

## Requirements

- Rust 1.96 or newer
- An [OpenRouter](https://openrouter.ai/) API key for terminal modes

## Setup

Clone the repository and create a `.env` file in its root:

```dotenv
OPENROUTER_API_KEY=your_api_key_here
```

`OPENROUTER_BASE_URL` is optional and defaults to `https://openrouter.ai/api/v1`.

Build the project:

```sh
cargo build --locked
```

## Usage

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

The agent currently uses `anthropic/claude-haiku-4.5` and can execute `Read`, `Write`, and `Bash` tool calls on the local machine. Run it only in a directory where you are comfortable allowing model-directed file changes and shell commands.

## Architecture

Hobgoblin keeps the agent core separate from its frontends:

- `src/agent.rs` owns the conversation and model/tool loop.
- `src/terminal.rs` provides one-shot and interactive terminal interfaces.
- `src/tools.rs` defines and executes local tools.
- `src/acp.rs` contains the developing ACP stdio transport.
- `src/wire.rs` defines provider-facing wire types.

The terminal frontend owns conversation state and passes it into the core for each turn. This allows one shared agent loop to support one-shot CLI, interactive CLI, and—eventually—ACP clients without coupling presentation or transport concerns to model execution.

## Development

Run the standard checks with:

```sh
cargo fmt --all -- --check
cargo check --locked
cargo test --locked
```

The near-term roadmap adds ACP initialization, session management, prompt turns, cancellation, permissions, and tool-call updates. The intended end result is a portfolio-scale Rust coding agent that runs inside Zed without relying on an ACP SDK.
