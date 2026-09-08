# Learning Log

Running notes from building Claude Code in Rust (CodeCrafters). Organized by
topic rather than by date, so it can grow into a proper wiki instead of a
transcript. Each entry aims to record **the mechanism and the why**, not just
the fix — a fix is worth one session, a mechanism is worth the rest of them.

**Adding entries:** put them under the relevant topic; start a new topic heading
when nothing fits. Date-stamp only when the entry is about a specific decision
we made, not for general knowledge.

---

## Unix streams

**stdout is fd 1, stderr is fd 2.** The convention: fd 1 carries the program's
*product*, fd 2 carries *commentary about producing it*. That's what makes
redirection useful — the two can be separated because they were separated by
design.

Redirection targets one fd at a time:

```bash
cmd 1>/dev/null     # see ONLY stderr
cmd 2>/dev/null     # see ONLY stdout
cmd 2>&1 | grep x   # merge stderr into stdout so a pipe sees both
cmd >out.txt 2>err.txt   # split into separate files
```

**Order matters in `2>&1 1>/dev/null`.** `2>&1` points fd 2 at wherever fd 1
*currently* is (the terminal), *then* fd 1 is redirected away. Net effect: only
stderr survives. Writing it the other way round gives the opposite. This trips
up nearly everyone.

**A pipe only carries fd 1.** `cmd | grep x` never sees error messages — they go
straight to the terminal. If a pipeline seems to be "swallowing" errors, it
isn't; it never received them.

Related: stdout is block-buffered when not attached to a TTY, stderr is
unbuffered. That's why CI logs sometimes appear interleaved out of order.

---

## Debugging method

**When output lands somewhere unexpected, ask which stream it's on before
asking what's emitting it.**

"What's emitting this?" is a search over the whole dependency tree — open-ended.
"Which fd is it on?" has exactly two answers, and each points at a different
file:

- On stderr → the redirect is wrong. Shell problem. Fix `run.sh`.
- On stdout → the emitter is misconfigured. Rust problem. Fix `main.rs`.

One command (`1>/dev/null`) discriminates. The general shape: **when a
hypothesis is expensive to confirm, look for a cheap observation that
eliminates the most possibilities.** Not "what's the answer" but "what's the
fastest question whose answer halves the search space."

Corollary: confirm the diagnosis *before* editing. `wc -l trace.jsonl` returning
0 proved the theory in one command; without it we'd have been guessing.

---

## `tracing` — architecture

**`tracing` is a facade; `tracing-subscriber` does the work.** The `info!` /
`debug!` macros emit events into the void. The subscriber collects, filters,
formats, and *writes* them. So anything about **where output lands is a
subscriber question, never a macro question.**

### Output stream — `MakeWriter`

`tracing_subscriber::fmt()` defaults its writer to **stdout**. That was the bug:
traces and `println!` output shared fd 1, so `2> trace.jsonl` captured nothing.

```rust
tracing_subscriber::fmt()
    .json()
    .with_writer(std::io::stderr)   // ← the fix (src/main.rs:98)
    .with_env_filter(...)
    .init();
```

The subscriber doesn't hold a writer, it holds a **factory** implementing the
`MakeWriter` trait, and calls it to get a fresh writer per event. There's a
blanket impl for `fn() -> W where W: io::Write`, which is why you pass
`std::io::stderr` **without parentheses** — you're handing over the factory, not
the handle.

Typing `std::io::stderr()` gives a trait-bound error that `Stderr: MakeWriter`
isn't satisfied. Worth triggering once on purpose; you'll recognize it forever.

### Filtering — `EnvFilter` / `RUST_LOG`

`RUST_LOG=debug` is **global** — it enables debug for every crate in the tree
that uses `tracing`. Our 2 events came with 4 lines of `reqwest` / `hyper_util`
noise. Scope it with per-target directives:

```bash
RUST_LOG=codecrafters_claude_code=debug,warn   # our crate at debug, everything else warn+
```

The target is the **crate** name with underscores (`codecrafters_claude_code`),
not the package name with hyphens. Common gotcha.

Fallback when `RUST_LOG` is unset is `"info"` (`src/main.rs:100`). Note: after
removing the startup `info!`, we have zero info-level events — so no `RUST_LOG`
now means an *empty* trace file, not a quieter one.

---

## `tracing` — the macro DSL

`debug!(...)` is a `macro_rules!` macro with **its own grammar**. What's inside
the parens is parsed by the macro, not by Rust's expression grammar. `=` is
**not** assignment and declares no variable.

```rust
debug!(count = 3);            // bare: type must impl tracing::Value
debug!(payload = %request);   // % → record via Display
debug!(payload = ?request);   // ? → record via Debug
debug!(n_messages);           // shorthand: field name from the variable name
debug!(http.method = "GET");  // dots are legal in field names
debug!(event = "x", "hello"); // trailing string literal → the `message` field
```

`field = value` means *record a field named `field`*. So `n_messages =
messages.len()` emits `"n_messages": 1` — a real JSON **number**, queryable with
`jq 'select(.fields.n_messages > 3)'` and no re-parsing.

`message` isn't special-cased anywhere; it's just the conventional name the
macro assigns to a bare trailing string. Our `event = "llm_request"` is an
ordinary field we chose to name `event`.

**Why a DSL?** The expansion checks the level *first* and only builds the field
set if the event is enabled — that's how a disabled `debug!` costs near-zero.
But it requires the macro to see field names as literal tokens at compile time,
hence the custom grammar.

### Why you can't log a nested JSON object

Fields reach the subscriber through `tracing::field::Visit`, whose methods are:

```rust
record_f64  record_i64  record_u64  record_bool  record_str  record_debug
```

Primitives and strings. **There is no `record_object`.** By the time the JSON
formatter sees `payload = %request`, it's already a flat string, and escaping it
is the only correct move. The nesting was lost one layer earlier than you'd try
to fix it.

This is deliberate — it's what lets a value be recorded with zero allocation on
the hot path. `%` and `?` exist precisely *because* most types aren't `Value`:
they tell the macro which formatting trait to funnel through.

`usize` **is** a `Value` (recorded as u64 — see
`tracing-core-0.1.36/src/field.rs:553`), so `messages.len()` works bare.

Escape hatches, in order of sanity:
1. Log many small typed fields instead of one blob (idiomatic).
2. Keep the blob, re-parse with jq's `fromjson` (fine — see below).
3. Implement `FormatEvent` yourself and splice in the parsed `Value` (an
   afternoon; also the machinery for adding span context / request-ids).
4. The `valuable` crate — unstable, needs `RUSTFLAGS="--cfg tracing_unstable"`,
   and `serde_json::Value` doesn't implement `Valuable` anyway. Skip.

---

## jq

**`~/.jq` is auto-loaded.** Definitions there are available in every invocation
with no flags. Good for things true across *all* projects.

**Per-project: modules via `-L`.** We keep `jq_defs.jq` in the repo root:

```bash
jq -L. -r 'include "jq_defs"; mine | brief' trace.jsonl
jq -L. 'include "jq_defs"; resp | .choices[0].message' trace.jsonl
```

- `-L.` adds a directory to the module search path.
- `include "jq_defs";` finds `jq_defs.jq` — the extension is implied.
- `include` must come **first**, before any filter.
- `import "jq_defs" as t;` namespaces instead (`t::payload`), for name
  collisions.

Current `jq_defs.jq`:

```jq
def payload: .fields.payload | fromjson;
def mine:    select(.target == "codecrafters_claude_code");
def brief:   "\(.level) \(.target) \(.fields.event // .fields.message)";
def req:     mine | select(.fields.event == "llm_request")  | payload;
def resp:    mine | select(.fields.event == "llm_response") | payload;
```

`-L.` resolves against the *current* directory, so it breaks in subdirectories.
Optional wrapper script to fix that:

```bash
#!/usr/bin/env bash
root=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
exec jq -L "$root" "$@"
```

(`exec` replaces the shell process so signals and exit codes pass through
cleanly — standard for argument-adding wrappers.)

### Viewers

- `jless` — `less`-style pager built for JSON, reads JSONL natively, collapsible
  nodes, `/` search. **Not yet installed.**
- `fx` — mouse-driven explorer.
- `duckdb` — `SELECT * FROM read_json_auto('trace.jsonl')`, i.e. SQL over the
  log. Gets attractive once there are hundreds of records per run.
- `bat -l json` — syntax highlighting for piped output (installed).

---

## Process

**2026-08-07 — teaching mode.** `AGENTS.md` now states operationally what "act
as a teacher" means: don't edit `src/`, give background + step-by-step +
rationale, provide a way to verify the diagnosis before changing anything.
The original one-liner didn't bind because it wasn't specific.

---

## ACP learning track

**2026-09-08 — course setup, not mastery evidence.** Target: expose this agent to Zed over local stdio using Agent Client Protocol (ACP). Keep the existing CLI. Peter writes the Rust; Hermes provides explanations, bounded exercises, and verification.

- [Project course](guides/acp/README.md): API-focused lessons and source-specific integration seams.
- [Progress ledger](guides/acp/progress.md): introduced/practiced/demonstrated states with learner evidence; no ACP concepts assessed yet.
- Reusable reference: `/Users/peter/knowledge-bundles/harness-engineering/`, extended rather than duplicated.
- Start with [Lab 00](guides/acp/00-inspect.md). The guide's handshake example was compiled and probed separately; no ACP adapter has been added to this project's source.

## Open threads

- [ ] Flatten `llm_request` / `llm_response` into typed fields (`model`,
      `n_messages`, `finish_reason`, token counts) — `src/main.rs:151,159`.
- [ ] Scope `RUST_LOG` in `run.sh` to drop the reqwest/hyper noise.
- [ ] `brew install jless`.
- [ ] Stretch: custom `FormatEvent` for genuinely nested payloads.
