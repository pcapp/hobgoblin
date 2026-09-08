# Learn ACP by connecting this Rust agent to Zed

> **Scope:** teach and verify; Peter writes the Rust. The current program is not yet an ACP agent.
> **Target:** local Zed over stdio, stable ACP v1, Rust SDK documentation pinned to 2.1.0.
> **Start:** [Development milestones](milestones.md) → [Lab 00: short preflight](00-inspect.md) → [Lab 01: initialize](01-handshake.md).

## What you are building

Keep the existing CLI and add a protocol adapter around the agent, not around its terminal output. Zed supplies user prompts and displays updates; your agent still owns model calls, history, tool policy, and execution. A turn can contain several model requests; a session contains several turns.

```text
CLI input ──→ CLI adapter ─┐
                          ├─→ session / turn logic ─→ provider and tools
Zed ⇄ stdio ⇄ ACP adapter ─┘           │
                          adapters ←── application events and outcomes
```

**Proposed design, not current code:** the turn logic emits typed application events; adapters choose how to present them. CLI can print an answer. ACP serializes a message update and finally a prompt response. Do not make provider/tool code know Zed UI details.

## Reference shelf

The shared reference lives outside this repo in the existing OKF wiki, not in a duplicate course wiki:

- [ACP mental model](file:///Users/peter/knowledge-bundles/harness-engineering/concepts/acp-mental-model.md)
- [API map and annotated JSON exchange](file:///Users/peter/knowledge-bundles/harness-engineering/protocols/acp-api-map.md)
- [Rust packages, types, and async pitfalls](file:///Users/peter/knowledge-bundles/harness-engineering/references/acp-rust-sdk.md)
- [Permissions, tools, and cancellation](file:///Users/peter/knowledge-bundles/harness-engineering/concepts/acp-tools-and-cancellation.md)
- [Teaching and evidence policy](file:///Users/peter/knowledge-bundles/harness-engineering/workflows/acp-learning.md)
- [Obsidian now, Starlight later](file:///Users/peter/knowledge-bundles/harness-engineering/references/acp-wiki-viewing.md)

If your editor blocks `file:` links, open `/Users/peter/knowledge-bundles/harness-engineering/` as an Obsidian vault. The wiki itself uses relative Markdown links.

## Where this project stands

Source locations were checked when this guide was created. Re-find symbols after edits rather than trusting stale line numbers.

| Existing code | Why it matters for ACP | Future learning change |
|---|---|---|
| `src/main.rs:11–14` required `prompt` | Zed launches a long-lived process, not one `-p` invocation | Separate CLI input from ACP startup |
| `src/main.rs:18–24` stderr tracing | Already compatible with protocol-only stdout | Preserve this, redact payload logging |
| `src/main.rs:28–44` provider setup | Echo/handshake need no API key; launch cwd may differ | Keep protocol setup independent from provider auth |
| `src/agent.rs:7–15` `run` and local history | Repeated calls lose conversational state | Let a session own history across turns |
| `src/agent.rs:31` awaited provider call | Whole-response provider API, not token streaming | A single ACP text chunk is a valid first bridge; token streaming is a separate improvement |
| `src/agent.rs:39–50` malformed/no-choice returns | Errors currently return `Ok(())` | Use explicit domain errors and map them deliberately |
| `src/agent.rs:59–64` `println!` | Raw answer is invalid ACP stdout | Emit a domain event or return structured output |
| `src/agent.rs:66–77` synchronous tools | No UI progress, approval, or cancellation boundary | Introduce a tool-execution boundary before exposing it to Zed |
| `src/agent.rs:81–86` loop exhaustion | Returns success without a meaningful turn outcome | Distinguish `max_turn_requests` |
| `src/tools.rs:73,96` local disk I/O | Does not reflect unsaved editor buffers | Evaluate client filesystem delegation |
| `src/tools.rs:120–137` blocking process output | No exit-success check; JSON byte arrays; no session cwd | Design exit-aware, cancellable execution and deliberate text conversion |
| `src/wire.rs:4–29` provider DTOs | These are not ACP messages | Keep provider decoding types separate from SDK schema types |

## Development milestones and step-by-step guides

Start with the [milestone roadmap](milestones.md): outputs, learning objectives, test layers, review procedure and the first small finish line.

- [00 — Inspect the current boundary](00-inspect.md)
- [01 — Initialize without a model](01-handshake.md): typed API example and supplied no-key verification script.
- [02 — Start in ACP mode and echo into Zed](02-echo.md): CLI routing, session state, update ordering and early cancellation.
- [03 — Connect session-owned turn logic](03-turn-engine.md): provider fakes, explicit outcomes and CLI preservation.
- [04 — Expose tools safely](04-tools.md): permission, capability, workspace and execution tests.
- [05 — Cancel under load](05-cancellation.md): provider/approval/process cleanup and race tests.
- [06 — Verify the supported profile](06-compatibility.md): baseline resource links, stdio MCP and Zed acceptance evidence.

Each lesson separates concepts, files/actions, API examples, tests you write and the review gate. Later paths are proposed design targets; re-read actual code before applying changes. Rust test targets described in lessons are assignments, not already implemented tests. Hermes supplies guidance and review tooling, not application Rust.

## Zed setup: only after session/prompt works

Zed's documented path is Agent Settings → External Agents → Add Agent → Add Custom Agent. It creates an `agent_servers` entry. Use the absolute path to your built executable and arguments matching your implementation; do not point Zed at today's CLI and expect ACP.

For a standalone example, build it with `cargo build --locked --example acp_handshake` and configure the resulting absolute executable path **only after** you extend it beyond the handshake. The future integrated binary may instead use an explicit `--acp` mode; no such flag exists today.

Use Command Palette → `dev: open acp logs` to inspect traffic. The installed Zed version at guide creation was **1.17.2**; actual wire interoperability has not been tested. GUI processes may not inherit shell environment setup. Keep provider credentials out of versioned Zed examples and out of ACP logs.

Source: [Zed External Agents](https://zed.dev/docs/ai/external-agents).

## How we record learning

Use [progress.md](progress.md). Explain the mechanism, attempt the exercise, then ask Hermes to verify. A passing assistant-run build is a baseline observation, not evidence that you have mastered the concept. At session end, record one mechanism learned, one piece of evidence, and the next open question.

**Suggested next message:** “Start ACP Lab 00. Ask me the boundary questions one at a time.”
