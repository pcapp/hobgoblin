@AGENTS.md

# Notes for Claude Code

## Two agents, two roles

I do the hands-on coursework with **Zed's built-in agent** (a GPT model), following the
teaching rules in AGENTS.md. Sessions with Claude Code are for **stepping back**: reviewing
the architecture, the course plan, how the Zed sessions are going, and the portfolio angle.
The teaching rules still apply here: don't edit `src/`. Guides, scripts, config and
docs are fine to edit when I ask.

## Reading the Zed sessions

Zed stores agent threads in `~/Library/Application Support/Zed/threads/threads.db`
(zstd-compressed JSON), not in `~/.codex/sessions`. Use:

```sh
scripts/zed-threads.sh              # list this repo's threads, newest first
scripts/zed-threads.sh <id-prefix>  # render one thread as Markdown
```

When I ask "where am I" or "how's it going", read `guides/acp/progress.md`, check `git log`
and `git status`, then read the latest one or two threads. Code discussed in a thread may
not have been typed in yet, so check the source before assuming.

## Where the Rust learning is heading

Typing Rust by hand is how I learn it, not the end goal. The goal is enough fluency to
delegate Rust to an agent, as I already do with TypeScript and Python. When I ask whether
I'm ready to hand off more, check these four things:

1. **Read:** I can review an agent's Rust diff and spot ownership, error-handling, and
   async mistakes, such as a lock held across `.await` or errors swallowed instead of returned.
2. **Specify:** I can describe work in Rust terms, such as the types, who owns what, and the
   trait boundaries.
3. **Debug:** I can work through compiler errors and runtime failures without the agent.
4. **Idiom:** I recognise when code is un-Rusty even if it compiles.

Proposed progression (not yet decided): I type everything for now. Then the agent gives
goals and hints and I write the code. Eventually I design and specify, the agent writes,
and I review until I can defend every line.

## Observations so far (2026-09-29)

- **Teaching style that works:** one new Rust concept per step, short answers, and the
  compiler and tests make each concept concrete. I read *Learn Rust in a Month of Lunches*
  alongside the course.
- **Harness concepts covered:** conversation vs session, the turn vs the inner agent loop,
  frontend-owned state, a `Model` trait as a test seam with a scripted model, and `Write`/`BufRead`
  as I/O seams.
- **Gap in the ACP plan:** `milestones.md` stops at `initialize` and defers `session/new`,
  `session/prompt`, updates, tool calls, permissions and cancellation. Those are what make the
  agent usable in Zed and what make it a good portfolio piece. Plan tasks 4 and beyond before
  Task 3 finishes.
- **Portfolio gaps:** the repo name (`codecrafters-claude-code-rust`) and README are still the
  CodeCrafters template. Stray files (`pbcopy`, `hello.txt`, `execute_the_read_tool_refactor.md`)
  need cleaning up. The target demo is "my Rust ACP agent running in Zed's agent panel", which
  also supports my Zed contribution plans.
- **Zed agent friction:** it sometimes stops after announcing an action, and it reruns the full
  acceptance suite for small questions.
