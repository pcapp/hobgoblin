# Task 6b — Clean up the ACP module

> **Inserted between Task 6 and Task 7.** Task 7 (cancellation) is paused partway through; resume it after this task. The cancellation code already in `src/acp.rs` moves with everything else. Do not finish it here.

## End goal

`src/acp.rs` (2,376 lines, about 1,700 of them tests) becomes a small module tree where each file has one job. `dispatch` becomes a short router. Request failures get a typed error. The tests read as scenarios rather than JSON dumps.

**Behavior does not change.** Every existing test still passes, and the black-box ACP suite passes unchanged. Fixing bugs is out of scope; they are listed at the end with the milestone that owns them.

## Order

Do the steps in this order. Each step ends with `./premerge.sh` green, and should be its own commit.

1. **Test support first.** Shorter tests are cheaper to move and protect every later step.
2. **Make `dispatch` delegate.** This is easiest while everything is still in one file.
3. **Add a typed request error.** This removes most of what remains in the handlers.
4. **Split the file.** By now the move is mechanical: small functions going to their new homes.

---

## Step 1 — Test support

### Problem

- `should_cancel_active_prompts` is 156 lines; Clippy's `too_many_lines` flags it.
- The `{"choices": [{"message": ...}]}` reply blob is written out by hand in `acp.rs`, `agent.rs` and `terminal.rs`.
- A scripted `Model` is defined 3 times across modules and 4 more times inside individual acp tests.
- 12 tests repeat the same 6-line `duplex` / `split` / `BufReader` / `BufWriter` setup, and 13 build a `Cursor` input by hand.
- Prompt-request JSON is copied into every prompt test.

### Concepts

**`#[cfg(test)]` modules shared across the crate.** A module declared in `main.rs` as `#[cfg(test)] mod test_support;` is compiled only for tests, and every other module can use it as `crate::test_support::...`. Items inside need `pub(crate)` to be visible outside the module.

**Helpers that hide wire shape.** A test should state *what* happens ("prompt 4 is cancelled") and leave the JSON layout to a helper. When the wire shape changes, only the helper changes.

### Contract

Create these and rewrite the tests to use them. The names are suggestions.

| Helper | Input | Output / effect |
|---|---|---|
| `assistant_reply(text)` | answer text | the provider response `Value` with one choice and no tool calls |
| a shared scripted model | a list of replies | records each request; returns replies in order |
| `prompt_request(id, session_id, text)` | request id, session, prompt text | the `session/prompt` frame |
| `cancel_notification(session_id)` | session | the `session/cancel` frame |
| `agent_message_chunk(session_id, text)` | session, text | the `session/update` frame you expect to receive |
| a test client | a model | starts `run_acp_with_model` over a duplex pipe; `send(&Value)`, `recv() -> Value`, `shutdown()` that also joins the agent task |

Example of the target: the cancellation test should read roughly as *initialize → new session → prompt 3 answered → prompt 4 started → cancel → prompt 4 cancelled → prompt 5 answered → history excludes prompt 4*, with each arrow about one line.

### Before implementing, decide

1. How does the shared scripted model express "block on this reply until the test releases it"? Today one test uses two `Notify`s and a turn counter. Options include a reply enum, a separate gated model, or a per-reply gate.
2. Do the `Cursor`-based tests (send all frames, read to EOF) and the duplex tests (interactive) share one client type, or does each get its own helper?

### Scope adjustment — 2026-11-12

The shared test client and Cursor setup helpers are deferred until additional interactive tests make the duplication worth abstracting. Only two current tests clearly fit the proposed client; transport-specific tests may need direct access to their plumbing. The choice between shared or separate Cursor/duplex helpers remains undecided. Existing frame helpers stay in place.

Step 1's implementation scope is the shared scripted model and the four message-building helpers. Separate test-only `scripted_model` and `test_messages` modules satisfy the shared-support goal.

### Acceptance

- `cargo clippy --locked --all-targets -- -W clippy::too_many_lines` reports no test-function warnings. The existing `dispatch` warning is deferred to Step 2; the full cleanup must finish warning-free.
- There is one scripted model definition in the crate: `grep -rn 'impl.*Model for' src` lists only the two real models and the shared test ones.
- The test-name check below passes.

---

## Step 2 — Make `dispatch` delegate

### Problem

`dispatch` (`src/acp.rs:474`, 198 lines) is an `if`/`else` chain on method strings. The `session/prompt` branch contains the whole spawned task body, nested five levels deep. `read_frames` handles `session/cancel` inline.

### Contract

`dispatch` keeps its signature and becomes a router: one `match request.method.as_str()` with one arm per method, each arm calling a handler. Target: under 30 lines.

| Function | Responsibility |
|---|---|
| `handle_initialize` | parse params, mark the connection initialized, build the result |
| `handle_new_session` | parse params, validate `cwd`, register the session, build the result |
| `handle_prompt` | parse params, look up the session, spawn the turn; return no immediate response |
| the spawned turn body | its own `async fn` returning `Result<(), AcpError>`, so `tokio::spawn` gets a one-line call |
| `handle_cancel` | the notification logic currently inline in `read_frames` |

While you are in there:

- `serde_json::to_value(response)?` appears 9 times. Give responses one serialization point.
- `jsonrpc: String::from("2.0")` appears 4 times. Move it into a `SuccessResponse::new(id, result)` constructor.
- `"sessionId": &session_id.0.to_string()` can be `session_id`, because `SessionId` already serializes as a plain string.
- `read_frames` parses each frame twice: `from_str` into `Value`, then `from_str` again into `Request` or `Notification`. Use `serde_json::from_value` on the first parse.
- The two identical abort-then-join blocks in `run_acp_with_model` can share a helper.
- `Request.method` and `Request.params` are both used now, so remove their `#[allow(dead_code)]`.

### Before implementing, decide

1. Today only `session/new` checks `state.initialized`; `session/prompt` before `initialize` returns `-32002`. Checking once at the top of `dispatch` for every method except `initialize` changes that response to `-32001`. Keep today's behavior during cleanup, or make the change with a test that pins it?
2. What do handlers return: a ready `Value`, or something `dispatch` serializes? (Step 3 may answer this for you.)

### Acceptance

- `dispatch` is under 30 lines and has no nested `match` deeper than one level.
- `read_frames` contains no method-specific logic.
- The test-name check passes.

---

## Step 3 — Typed request errors

### Problem

- Every handler failure is built by hand at the failure site: `ErrorResponse::invalid_params(request.id)`, then `serde_json::to_value`, then `return Ok(Some(...))`. That is most of `dispatch`'s length.
- `parse_prompt_params` returns `Result<_, ()>`, which throws away *why* parsing failed.
- `Box<dyn Error + Send + Sync>` is used everywhere, so no caller can tell a bad request (answer it) from a broken pipe (shut down).

### Concepts

**Two kinds of failure.** A *request error* is the client's problem: invalid params, unknown session, unknown method. It becomes a JSON-RPC error response, and the connection keeps going. A *transport error* is fatal: stdin closed mid-frame, stdout broken, frame too large. It ends `run_acp`. Today both travel as `Box<dyn Error>`, so the code can't treat them differently by type, only by where it returns.

**An error enum.** Model the failures a caller can react to as variants. The caller can `match` on them; a `Box<dyn Error>` can only be printed.

**How `?` converts.** On an `Err(e)`, `?` returns `Err(From::from(e))`. If your error type implements `From<OtherError>`, `?` converts automatically. That is the entire mechanism; `thiserror` only generates these impls for you.

A small example, unrelated to ACP:

```rust
#[derive(Debug)]
enum ConfigError {
    Missing(&'static str),
    Io(std::io::Error),
}

impl From<std::io::Error> for ConfigError {
    fn from(error: std::io::Error) -> Self {
        ConfigError::Io(error)
    }
}

fn load() -> Result<String, ConfigError> {
    let text = std::fs::read_to_string("app.toml")?; // io::Error -> ConfigError
    if text.is_empty() {
        return Err(ConfigError::Missing("app.toml"));
    }
    Ok(text)
}
```

To fit into `Box<dyn Error>` (for example, returned from `main`), a type also needs `impl std::fmt::Display` and `impl std::error::Error`. Write these by hand once. Afterwards, [`thiserror`](https://docs.rs/thiserror) can derive them; it is the usual choice for library-style error enums.

**Keep `Box<dyn Error>` at the top.** `main` and other "print it and exit" boundaries are the right place for a boxed error. Typed errors matter where a caller makes a decision.

Reading: *The Rust Programming Language*, chapter 9.2 ("A Shortcut for Propagating Errors: the `?` Operator"); `std::convert::From` in `cargo doc --open`.

### Contract

- **Target:** a request-error enum in the ACP module.
- **Variants:** the request failures `dispatch` produces today: invalid params, connection not initialized, session (resource) not found, method not found, internal.
- **Handlers** return `Result<…, YourRequestError>` and use `?` for parsing. For example, a Serde failure maps to invalid params.
- **One place** turns an `Err` into an `ErrorResponse` with the same code and message the client gets today.
- `parse_prompt_params` returns the request error instead of `()`.

Optional, only if you want the second half of the lesson: a transport-error enum (I/O, JSON, frame too large, missing newline, invalid message shape) with `From` impls, replacing the `std::io::Error::new(InvalidData, …)` calls in the framing code.

### Before implementing, decide

1. Where does a variant become an `ErrorResponse`: a method on the enum, a `From` impl, or a `match` in `dispatch`? Note that the response needs the request `id`, and the error does not have it.
2. Does `serde_json::to_value` failing on a response count as a request error (`-32603`) or a fatal transport error?

### Acceptance

- No handler calls `ErrorResponse::…` directly; one conversion site does.
- No `Result<_, ()>` remains in `src/`.
- Existing error-code tests (`-32602`, `-32601`, `-32001`, `-32002`) pass unchanged.

---

## Step 4 — Split `acp.rs`

### Concepts

**File layout.** `mod acp;` in `main.rs` loads either `src/acp.rs` or `src/acp/mod.rs`. Both styles let `acp` have children (`src/acp/transport.rs` is `acp::transport`). Pick one and stay consistent.

**Visibility.** A private item is visible in its own module and in that module's children. That is why `mod tests` inside `acp.rs` can call private functions today. Sibling modules (`acp::handlers` calling `acp::registry`) need `pub(super)` or `pub(crate)`. Expect the compiler to tell you each one.

**Unit tests move with their code.** A `#[cfg(test)] mod tests` at the bottom of each file tests that file. End-to-end duplex tests that drive the whole connection go in their own child module.

### Suggested layout

```text
src/acp/
  mod.rs        pub run_acp, run_acp_with_model; spawns reader and writer; the reader loop
  transport.rs  MAX_FRAME_BYTES; read one frame; write_frame, write_frames; classify_message; validate_*
  protocol.rs   JSON-RPC and ACP wire types, error codes, SuccessResponse/ErrorResponse, prompt parsing
  registry.rs   SessionId, SessionEntry, ConnectionState, get_session
  handlers.rs   dispatch and the per-method handlers, including cancel and the prompt task
  error.rs      the request-error enum from Step 3
  tests.rs      end-to-end duplex tests and the test client
```

Use `registry`, not `session`, to avoid confusion with `crate::session`.

The one non-obvious seam is the reader loop. Today `read_frames` does framing *and* routing. Here, `transport` returns one parsed frame (or `None` at EOF) and knows nothing about methods; the loop in `mod.rs` hands each frame to `handlers`. That way `transport` never depends on `handlers`.

### How to move safely

Move one file at a time, leaf modules first (`protocol`, `registry`, then `transport`, then `handlers`). Run `cargo check` after each. Move each function's unit tests in the same commit as the function.

### Acceptance

- `src/acp.rs` no longer exists as a 2,000-line file; no file under `src/acp/` is over about 500 lines including its tests.
- The test-name check passes.

---

## Validation for every step

Run this before Step 1 and keep the output:

```sh
cargo test --locked -- --list 2>/dev/null | grep ': test$' | sed 's/.*:://; s/: test$//' | sort > /tmp/hobgoblin-tests-before.txt
```

After each step, run the same command into `/tmp/hobgoblin-tests-after.txt`, then:

```sh
diff /tmp/hobgoblin-tests-before.txt /tmp/hobgoblin-tests-after.txt   # no output: no test lost or renamed
./premerge.sh                                                          # fmt, Python lint, clippy, Rust tests
cargo build --locked && uv run --project guides/acp/acp_test_suite \
  guides/acp/acp_test_suite/test_handshake.py \
  --report /tmp/acp-test-suite.json \
  --executable target/debug/hobgoblin --acp                            # black-box suite: 8 of 8 on 2026-11-12
```

`premerge.sh` only lints the Python suite; it does not run it, so the last command is the black-box check.

The `sed` drops module paths, so tests that move from `acp::tests` to `acp::handlers::tests` still match. If you intentionally rename or add a test, update the before-file and say so in the ledger.

---

## Not in this task

### Known bugs, deferred to their milestones

| Bug | Where | Owner |
|---|---|---|
| Cancel can't interrupt a running tool; blocking `std::process` ties up a runtime thread | `tools.rs:120`, `agent.rs:146` | Task 7 (already in its guide) |
| A `session/cancel` that arrives before the prompt task installs its stop sender is dropped | `acp.rs:566-573` | Task 7 |
| The spawned prompt task's `Result` is discarded, so its errors vanish | `acp.rs:565` | Task 7, or Step 2 here if the extracted function makes it a one-liner |
| Bash tool returns `stdout`/`stderr` as byte arrays; `ok: true` ignores the exit status | `tools.rs:134-138` | Task 9 (tool call updates), or earlier |
| Tools run in the process directory, not the session's `cwd` | `session.rs:15`, `tools.rs` | Task 8 or 9 |
| Bash tool error says "Invalid Write arguments"; "Could run" is missing "not"; Write returns `"content": null` | `tools.rs:114`, `:129`, `:97` | Any time |

### Other cleanup, prioritized

**Do soon. Cheap, and visible on a public repo:**

1. Add a `LICENSE` file, and `license` and `repository` fields in `Cargo.toml`.
2. Review `CLAUDE.md` for anything you don't want public. It currently includes portfolio strategy and Zed contribution plans.

**Later, when it gets in the way:**

3. One model client. The terminal builds a raw `Client` in `main.rs:51-63`, while ACP uses `OpenRouterModel`, which repeats the same environment setup (`agent.rs:51-69`). Using `OpenRouterModel` for both removes the duplicate.
4. `eprintln!` in `agent.rs:149` mixes plain text into the JSON log stream on stderr; use `tracing`.
5. The model name is hardcoded at `agent.rs:108`.
6. `Conversation.messages` is `pub`, and ACP truncates it directly. A rollback method would keep that invariant inside `Conversation`.
7. ACP sessions use `Session::empty`, so skills load in the terminal but not in Zed.
8. Neither CI nor `premerge.sh` runs the black-box ACP suite; both only lint it. Adding the run command to `premerge.sh` is a one-line change.

**Skip for now:**

9. CodeCrafters leftovers (`.codecrafters/`, `codecrafters.yml`, `your_program.sh`), pedantic Clippy warnings, and a Zed demo GIF. The GIF belongs to the Task 9 portfolio demo.
