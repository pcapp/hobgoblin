# Grow this Rust agent into an ACP agent

This course changes the existing application directly. There is no separate example agent and no throwaway protocol implementation.

You will evolve one shared agent core that can support three frontends:

```text
                         shared agent core
                    conversation + model/tool loop
                         /       |       \
                        /        |        \
             one-shot `-p`   interactive   ACP over stdio
```

The core owns agent behavior. Each frontend owns its input and presentation:

- `-p "prompt"` performs one turn and exits.
- Running without arguments starts a multi-turn terminal conversation.
- `--acp` lets an ACP client drive the same application.

## Working agreement

- You design and write the Rust.
- Each task has a concrete end state and a way to validate it.
- Concepts and constraints are explained when they become relevant, rather than in a separate architecture lesson.
- The course does not require test-driven development, but every task ends with deterministic automated acceptance checks.
- ACP is implemented with the project's normal Rust tools, including Serde and Tokio. Do not use an ACP SDK or an external ACP schema crate.
- Keep provider API types separate from ACP wire types.

## Current task sequence

1. [Extract a reusable agent core](01-shared-core.md)
2. [Add an interactive multi-turn terminal](02-interactive-cli.md)
3. [Add ACP initialization to the application](03-acp-initialize.md)

These tasks stop at initialization. Session creation, ACP prompts, tool reporting, permissions, and cancellation will be designed after the code has evolved through these first changes.

See [the roadmap](milestones.md) for the goals and boundaries of the current sequence. Record completed work in [progress.md](progress.md).

## Target architecture

`src/main.rs` chooses a frontend at startup. The frontend creates or locates conversation state and asks the shared core to perform a turn. The core must not know whether its caller is a terminal or an ACP client.

Do not create modules merely to match a diagram. Add a boundary when a task gives it a concrete responsibility.

## Validation philosophy

A successful build proves that the Rust types fit together; it does not prove behavior. Completion is based only on commands an assistant or CI process can run:

- Deterministic Rust tests with scripted dependencies for core and terminal behavior.
- Compile, formatting, and regression commands.
- A subprocess wire checker for ACP initialization.

Live model behavior, manual terminal interaction, and written explanations are not completion evidence.
