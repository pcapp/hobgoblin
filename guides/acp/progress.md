# ACP learning progress

This ledger tracks the current hands-on task sequence. The previous architecture-first and standalone-example course has been retired.

## Working decisions

- Evolve the existing application; do not build a separate example agent.
- One shared core supports one-shot CLI, interactive CLI, and ACP frontends.
- Keep `-p` as one-shot mode.
- Running without arguments starts an interactive multi-turn conversation.
- `--acp` starts the ACP frontend.
- Implement ACP directly with Serde and Tokio.
- Do not use an ACP SDK or external ACP schema crate.
- Use high-level, goal-driven tasks with machine-verifiable acceptance checks.
- The learner owns the design and writes the Rust.

## Task status

| Task | Status | Goal | Evidence |
|---|---|---|---|
| [1 — Shared core](01-shared-core.md) | complete | Caller-owned history; returned answer; `-p` preserved | Four deterministic behavioral tests pass; all acceptance commands exited 0 on 2026-09-29 |
| [2 — Interactive CLI](02-interactive-cli.md) | complete | Multi-turn terminal conversation; `/exit`, `/quit`, and EOF | All required parser and interaction behaviors pass; all acceptance commands exited 0 on 2026-10-05 |
| [3 — ACP initialize](03-acp-initialize.md) | complete | Real binary completes ACP initialization through Tokio stdio | All 41 tests and all 7 external handshake cases passed; every acceptance command exited 0 on 2026-10-09 |
| [4 — Reader and writer tasks](04-reader-writer-tasks.md) | in progress | Preserve ACP behavior with separate input and single-owner output tasks | Channel and writer loop validated; spawned tasks and duplex acceptance tests remain |

Use `not started`, `in progress`, `complete`, or `blocked`. Mark a task complete only when every listed command exits with status 0 and its required automated cases are present in the test/checker output.

## Session checkpoint — 2026-09-29

### Current understanding

- `Conversation` owns the ordered provider-visible message history and keeps its messages private.
- The terminal frontend owns each conversation; `agent::turn` temporarily borrows it as `&mut Conversation` and retains user, assistant, and tool messages.
- `agent::turn` returns final assistant text rather than printing it, allowing each frontend to choose its presentation mechanism.
- `terminal::run_once` accepts a `Write` output boundary, so production can use stdout while tests use an in-memory `Vec<u8>`.
- A small `Model` trait allows deterministic scripted tests without network requests or credentials.

### Source checkpoint

- Task 1 is complete and verified.
- `src/agent.rs` contains the caller-owned conversation boundary and three agent behavior tests.
- `src/terminal.rs` contains one-shot and interactive entry points plus the one-shot output test.
- `src/main.rs` passes stdout to one-shot mode.
- The working tree has uncommitted changes in `src/main.rs`, `src/terminal.rs`, and this ledger.

### Exact resume point

1. Read `02-interactive-cli.md` and compare its acceptance criteria with the existing `run_interactive` scaffold.
2. Verify the current EOF behavior before changing code.
3. Introduce only the input/output seams needed for deterministic interactive tests.
4. Implement `/exit`, `/quit`, EOF, and multi-turn behavior one requirement at a time.

Rust principles practiced in Task 1: model state with a struct, preserve invariants with private fields, express temporary mutation with `&mut`, return data instead of performing caller-specific effects, use trait bounds at genuinely variable boundaries, and inject `Write` for testable output.

## Session checkpoint — 2026-10-01

### Current understanding

- Clap models a presence flag such as `--acp` with `bool`; `false` means absent and `true` means present.
- `run_interactive` owns one `Conversation` outside its read loop, so later turns include earlier user and assistant messages.
- Injected `BufRead` and `Write` boundaries make the entire interactive transcript deterministic, including prompts and the final prompt before EOF or an exit command.
- Terminal transcript assertions prove presentation behavior, while recorded model-request assertions prove turn count and shared conversation history.
- `/exit`, `/quit`, and EOF must be handled by the outer terminal loop before another call to `agent::turn`.

### Source checkpoint

- Task 2 implementation and nearly all required deterministic coverage are present.
- The branch is `master`, two commits ahead of `origin/master`, with an uncommitted change in `src/terminal.rs`.
- On 2026-10-01, formatting, Cargo check, all 13 tests, and `cargo run --locked -- --help` exited 0.
- Task 2 is not yet complete: `interactive_quit_command_does_not_start_another_turn` is named for `/quit` but its input currently contains `/exit`.

### Exact resume point

1. In `src/terminal.rs`, change the quit test input from `b"Hello.\n/exit\n"` to `b"Hello.\n/quit\n"`.
2. Run the focused quit test, then all four Task 2 acceptance commands.
3. If every command exits 0, mark Task 2 complete and add its final validation record below.
4. Begin Task 3 by inspecting `src/main.rs`, `Cargo.toml`, `src/wire.rs`, and `guides/acp/check_handshake.py`; the first design decision is where ACP framing and dispatch should live.

Rust principles practiced in Task 2: model mutually exclusive CLI options with Clap, keep conversation state outside the outer loop, distinguish EOF from an empty line, inject synchronous I/O traits for deterministic frontend tests, and assert internal requests when output alone cannot prove retained state.

## Session checkpoint — 2026-10-05

### Current understanding

- Task 2's acceptance suite covers mode selection, parser rejection cases, shared interactive history, both exit commands, EOF, and one-shot behavior without a terminal, network, or API key.
- The `/quit` regression test now feeds `/quit`, so its passing result is direct evidence for that required behavior.
- All four acceptance commands must pass in the same verified tree before the milestone can be closed.

### Source checkpoint

- Task 2 is complete.
- The branch is `master`, three commits ahead of `origin/master`; uncommitted guide changes are present.
- On 2026-10-05, the focused `/quit` test and all four Task 2 acceptance commands exited 0; the full suite passed all 13 tests.
- No Task 2 work remains unverified.

### Exact resume point

1. Begin Task 3 by reading `03-acp-initialize.md`.
2. Inspect `src/main.rs`, `Cargo.toml`, `src/wire.rs`, and `guides/acp/check_handshake.py` before choosing where ACP framing and dispatch should live.
3. Verify the current behavior of `--acp` before making the first Task 3 change.

## Session checkpoint — 2026-10-05 (Task 3 started)

### Current understanding

- ACP initialization is a protocol concern and does not need a model provider or API key.
- `--acp` must be routed before dotenv loading and OpenRouter client construction.
- `BufRead::read_line` returns `0` when EOF occurs before any bytes are read, allowing clean EOF to end ACP mode successfully.
- Missing test coverage should be described by the behavior it must prove; the learner writes the test unless they ask for implementation help.

### Source checkpoint

- Task 3 is in progress.
- `src/main.rs` routes `--acp` before provider configuration.
- `src/acp.rs` treats empty stdin as clean EOF, with a focused regression test.
- Formatting, Cargo check, all 16 tests, a build, and a credential-free subprocess probe passed before this checkpoint was committed.

### Exact resume point

1. Continue replacing the synchronous ACP stdio scaffold with Tokio I/O.
2. Identify the Tokio features required for async stdin/stdout and buffered async reads.
3. Decide the async reader boundary needed by the next incremental change, then implement and verify only that change.
4. Bounded framing, response writing, dispatch, and full checker acceptance remain unverified.

## Session checkpoint — 2026-10-05 (Task 3 framing)

### Current understanding

- An `async fn` remains blocking if it calls synchronous I/O; ACP stdin now uses Tokio's `AsyncBufRead` boundary and `BufReader<Stdin>` in production.
- `read_line` appends to its `String`, so a reused frame buffer must be cleared before each read.
- `read_line` returns after a newline or EOF. Empty EOF is clean shutdown, while nonempty input ending at EOF without `\n` is an incomplete frame.
- A boxed `dyn Error` can be checked by downcasting to `std::io::Error` and then comparing its `ErrorKind`.
- A per-frame `take` adapter with a limit one byte above the policy can distinguish an exactly 64 KiB frame from an oversized frame without unbounded allocation.

### Source checkpoint

- Task 3 is in progress on `master`.
- ACP input uses Tokio async I/O, reads multiple bounded frames through clean EOF, and rejects an unterminated final frame as `UnexpectedEof`.
- `src/acp.rs` applies a per-frame `AsyncReadExt::take` adapter and rejects oversized input as `InvalidData`.
- Exact-limit and one-byte-over tests are present. The focused boundary tests, formatting, Cargo check, and all 20 tests passed.

### Exact resume point

1. Continue Task 3 with the ACP output boundary: decide just in time how `run_acp` should receive an async writer while preserving one stdout owner.
2. Add compact newline-terminated response writing and explicit flushing before expanding dispatch.
3. JSON-RPC classification and dispatch, initialization responses, unknown-method errors, and full checker acceptance remain unfinished.

## Session checkpoint — 2026-10-06 (CodeCrafters skills)

### Current understanding

- Skills are loaded once into a per-session `BTreeMap`, which gives deterministic name ordering and avoids filesystem access on every turn.
- `Session` owns both provider-visible conversation history and the loaded skill snapshot; `agent::turn` consumes shared session state, so terminal and future ACP prompts can use the same skill behavior.
- Skill resolution belongs in the shared agent path rather than `terminal.rs`, because terminal and ACP are separate frontends.
- A leading slash command resolves its first whitespace-separated token against the session snapshot; a recognized command replaces the provider-visible user prompt with only that skill's body.
- Skill discovery still exposes only names and descriptions in the initial system message, so uninvoked skill bodies are not sent to the model.

### Source checkpoint

- `src/session.rs` constructs one session from a `SkillLoader`, retains the resulting skills, and adds the level-one skill summary.
- `src/terminal.rs` creates one `Session` for one-shot or interactive use and no longer reloads skills separately.
- `src/agent.rs` contains `expand_skill_prompt` and calls it before adding the user message.
- `git status --short` reported no entries at checkpoint time.
- The skill-expansion changes have not been validated since the final implementation was typed; no new focused tests or CodeCrafters stage run were completed in this session.

### Exact resume point

1. Read `expand_skill_prompt` and manually confirm the intended policies for an ordinary prompt, a recognized `/skill`, and an unknown slash command.
2. Add focused coverage proving that only the invoked skill body reaches the model and that ordinary prompts remain unchanged.
3. Add lifecycle coverage proving that an interactive session calls `SkillLoader::load_skills` once.
4. Run `cargo fmt --all -- --check`, `cargo check --locked`, `cargo test --locked`, and the CodeCrafters `jd8` stage.
5. After the CodeCrafters work is verified, resume ACP Task 3 at the output-boundary decision recorded in the previous checkpoint.

## Session checkpoint — 2026-10-07 (Task 3 classification)

### Current understanding

- ACP stdio needs newline framing because an async byte stream does not preserve message boundaries.
- `AsyncBufRead` supports delimiter-oriented operations by retaining bytes across partial reads and preserving bytes after a delimiter.
- JSON-RPC request, notification, and response envelopes must be classified before method-specific deserialization; notifications have no ID and receive no response.
- A Tokio duplex stream can be modeled as two bounded directional byte queues. An empty open stream means "wait," while an empty closed stream means EOF.
- Flushing pushes bytes held by a buffered writer into its underlying async writer; it does not prove that the peer processed them.
- `Cursor` is the simplest seam for predetermined finite input, while `duplex` is useful when output or live two-way behavior must be observed.
- Future milestone work will happen on feature branches; strict formatting, Clippy, and test checks should gate completed work before it merges into `master`, while pull requests remain optional for this solo repository.

### Source checkpoint

- Task 3 remains in progress on `master` at commit `0b8c668`; the working tree was clean before this checkpoint entry.
- `src/acp.rs` has bounded async input framing, an injected async writer, compact newline-terminated frame writing with flushing, JSON-RPC shape classification, and a notification no-response regression test.
- `run_acp` parses each frame into `Value`, classifies it, validates request envelopes through the existing `Request<Value>` path, and ignores notifications and responses as required at this stage.
- `write_frame` is tested but is not yet connected to production dispatch, so the production writer parameter remains intentionally unused.
- The focused classifier suite passed all 10 tests earlier in the session. The complete acceptance suite and external handshake checker have not been run against the current tree.
- The blocking GitHub formatting and Clippy job was removed; GitHub CI now runs only the existing test matrix.

### Exact resume point

1. Establish the feature-branch workflow for the remainder of Task 3 and add one convenient local command or Zed task that runs the pre-merge quality checks.
2. Restore strict formatting, Clippy, and test checks for pull requests before merging the completed feature branch; configure a GitHub ruleset if those checks should technically block merges rather than merely report failures.
3. Demonstrate that a notification with `"jsonrpc":"1.0"` is currently accepted, confirming the common-envelope validation gap.
4. Add common JSON-RPC version validation for every classified message without moving method-specific validation into the shape classifier.
5. Add focused coverage for valid and invalid protocol versions, then run the ACP test subset.
6. Continue with `initialize` parameter validation and dispatch, connect `write_frame`, preserve string and numeric request IDs, and implement unknown-method `-32601` responses.
7. Finish Task 3 by running every command in `03-acp-initialize.md` and the subprocess handshake checker; record the report path and results.

## Session checkpoint — 2026-10-08 (Task 3 initialize response)

### Current understanding

- JSON-RPC version is a common-envelope invariant, so `run_acp` validates it once before request, notification, or response classification; method-specific request validation remains separate.
- `serde_json::Value::to_string` serializes JSON, including quotes around JSON strings; `Value::as_str` extracts a JSON string for comparison.
- `serde_json::to_value` converts a serializable Rust value into `Value`, while `serde_json::from_value` deserializes a `Value` into a Rust type.
- `SuccessResponse<T>` is a useful outgoing envelope because `T` can be the method-specific result while the JSON-RPC version and correlated ID remain common.
- A function returning `SuccessResponse<S>` cannot construct one concrete response body while promising an arbitrary caller-selected `S`; combining request, response, writer, and serialization generics made the increment too large to reason about.
- Wire-level tests should inspect the emitted `Value` independently rather than deserialize it with the same production type that serialized it.

### Source checkpoint

- Task 3 remains in progress on branch `initialize-acp` at commit `f059566`, tracking `origin/initialize-acp`; the working tree was clean before this ledger entry.
- `run_acp` now dispatches `initialize`, preserves the request ID, builds a `SuccessResponse<Value>`, and connects production dispatch to `write_frame`.
- The initialize response currently selects protocol version 1, emits an empty `agentCapabilities` object and empty `authMethods` array, and is covered by a wire-level response test.
- `cargo check --locked` and the focused initialize-response test exited 0 on 2026-10-08; the focused test passed 1 test.
- The test target still reports two warnings: unused imports for `serde_json::Value` and `SuccessResponse`. The response test also calls `validate_version` twice.
- Initialize params are not yet deserialized or validated. Unknown request methods are still ignored, and the complete Task 3 suite and handshake checker remain unverified.

### Exact resume point

1. Make only the no-behavior-change test cleanup in `src/acp.rs`: remove the unused `serde_json::Value` and `SuccessResponse` imports and one duplicate `validate_version` call.
2. Run `cargo fmt --all -- --check`, `cargo check --locked`, and the focused initialize-response test; confirm the warnings are gone.
3. Before adding more dispatch behavior, decide the smallest `InitializeParams` representation that enforces required `protocolVersion` and object-shaped `clientCapabilities` while accepting ACP extension fields.
4. Add focused missing/invalid initialize-params coverage, then deserialize params only inside the `"initialize"` branch.
5. Preserve the current working response before proceeding to string-ID coverage and unknown-method `-32601` responses.

## Session checkpoint — 2026-10-08 (Task 3 dispatch and client info)

### Current understanding

- Method dispatch is a useful boundary now that requests have two observable outcomes: `initialize` success and correlated `-32601` errors for unknown methods.
- Separate `SuccessResponse<T>` and `ErrorResponse` types prevent constructing a response with both `result` and `error` or with neither.
- `serde_json::from_value::<InitializeParams>` consumes a generic JSON value and applies the required/optional rules encoded by the target Serde type.
- `Option<Implementation>` accepts omitted or null `clientInfo`; when the object is present, plain `String` fields make `name` and `version` required while `Option<String>` keeps `title` optional.
- Unknown fields are ignored by Serde by default, which preserves ACP extensibility while the local type grows incrementally.
- `cargo test` builds test artifacts but does not guarantee that `target/debug/hobgoblin` is current; run `cargo build --locked` before launching the subprocess checker against that path.

### Source checkpoint

- Task 3 remains in progress on branch `initialize-acp` at commit `acf6f92`, tracking `origin/initialize-acp`; the working tree was clean before this ledger entry.
- `run_acp` delegates request handling to `dispatch`; initialize requests emit a typed success envelope, unknown methods emit correlated `-32601` errors, and malformed present `clientInfo` emits a correlated `-32602` error.
- The latest commit adds `InitializeParams` and `Implementation` only for optional `clientInfo` validation; `protocolVersion` and `clientCapabilities` are not yet represented by the params type.
- Diagnostics were clean. `./premerge.sh` exited 0 with formatting, strict Clippy, and all 38 tests passing.
- After `cargo build --locked`, the external handshake checker passed all 7 cases and wrote `/tmp/acp-initialize-review.json`.
- Task 3 is not complete: all explicit acceptance commands have not yet been run and recorded against the final tree, and initialize parameter validation remains incomplete.

### Exact resume point

1. In both error-response tests, change `"a error response must not contain an result"` to `"an error response must not contain a result"`; these assertion-message typos are the only review finding in commit `acf6f92`.
2. Add one red wire-level test proving that an initialize request missing required `protocolVersion` receives correlated `-32602 Invalid params`; keep valid `clientCapabilities` and omit `clientInfo`, which is optional.
3. Add `protocolVersion` to `InitializeParams` using the ACP schema's unsigned 16-bit range, then rerun the focused test and `./premerge.sh`.
4. Decide separately how to represent object-shaped `clientCapabilities` and its schema default before adding that field.
5. When parameter validation is complete, run every explicit command in `03-acp-initialize.md`, rebuild the binary, rerun the checker, and add the structured validation record.

## Session checkpoint — 2026-10-08 (Task 3 protocol version validation)

### Current understanding

- A required, non-`Option` Serde field makes a missing JSON property a deserialization error.
- `#[serde(rename_all = "camelCase")]` maps Rust's `protocol_version` field to JSON's `protocolVersion` property.
- `u16` enforces the ACP protocol version field's unsigned 16-bit range during deserialization.
- Invalid method parameters are represented by a correlated JSON-RPC `-32602` response; handling them successfully does not make `run_acp` return a Rust error.

### Source checkpoint

- `InitializeParams` now requires `protocol_version: u16`.
- A wire-level regression test proves that an initialize request missing `protocolVersion` receives correlated `-32602 Invalid params`.
- The two error-response assertion-message typos are corrected.
- The focused protocol-version test passed 1 test, and `./premerge.sh` exited 0 with formatting, strict Clippy, and all 39 tests passing.
- The complete Task 3 acceptance commands and external handshake checker have not been rerun against this working tree.

### Exact resume point

1. Decide how to represent object-shaped `clientCapabilities` and its ACP schema default without tightening extension handling.
2. Add focused coverage for the chosen required/default behavior before adding the field to `InitializeParams`.
3. After initialize parameter validation is complete, run every explicit command in `03-acp-initialize.md`, rebuild the binary, rerun the checker, and add the structured validation record.

## Session checkpoint — 2026-10-09 (Task 3 complete)

### Current understanding

- A field-level `#[serde(default)]` handles an omitted object property by calling that field type's `Default` implementation.
- Deriving Rust's `Default` does not by itself tell Serde to default missing fields inside a present object; `#[serde(default)]` must also be applied at the appropriate nested deserialization boundary.
- ACP's omitted `clientCapabilities` value defaults all known capabilities to disabled, while Serde's default unknown-field behavior preserves protocol extensibility.
- `cargo test` and the stricter `./premerge.sh` are distinct gates because the latter also runs Clippy with warnings denied.

### Source checkpoint

- Task 3 is complete on branch `initialize-acp` at commit `2978a97`; the working tree was clean before this ledger entry.
- `InitializeParams` validates required `protocolVersion`, optional `clientInfo`, and defaulted object-shaped `clientCapabilities` with nested filesystem, authentication, and terminal defaults.
- The missing-`protocolVersion` regression test remains alongside focused default and wire-level omission coverage for `clientCapabilities`.
- `./premerge.sh` exited 0 with all 41 tests passing.
- Every explicit Task 3 local acceptance command exited 0, and the external checker passed all 7 cases with report `/tmp/acp-initialize-review.json`.

### Exact resume point

1. Before starting Task 4 implementation, refine `04-reader-writer-tasks.md` against the retained Task 3 transport and tests, as required by the roadmap.
2. Begin Task 4 by reviewing its goal and the current `run_acp`, frame-reader, and frame-writer boundaries; no Task 3 work remains unverified.

## Session checkpoint — 2026-10-09 (Task 4 transport design)

### Current understanding

- One ACP stdio connection can eventually contain multiple concurrent operations that produce outbound messages; those are multiple message producers, not multiple stdout writers.
- A single writer task should exclusively own the output stream so async writes cannot interleave and corrupt newline-delimited JSON frames.
- A Tokio `mpsc` channel transfers owned messages from one or more `Sender` values to one `Receiver`; after every sender is dropped and queued messages are drained, `recv()` returns `None` and gives the writer a natural shutdown signal.
- For Task 4's sequential handling, the reader task should own the sender. Returning from the reader at EOF then drops the last sender, provided the coordinator does not retain another copy.
- A JSON-RPC error response is successful protocol handling and belongs in `Ok(Some(value))`; Rust `Err` is reserved for failures that prevent normal handling.

### Working Task 4 decisions

- The channel will carry complete `serde_json::Value` messages. This keeps Task 4 focused on task ownership, channels, and shutdown; a typed outgoing-message enum can be introduced later when real message categories justify it.
- Dispatch remains responsible for constructing complete JSON-RPC messages, using typed response structures before conversion to `Value` where useful.
- The writer remains structurally ignorant of message semantics and owns only compact serialization, newline framing, writing, flushing, and the output stream.
- The first refactor target is for dispatch to return `Result<Option<Value>, Error>`: `Some` means an outbound message, `None` means valid handling with no response, and `Err` means an internal failure.
- These decisions are intentionally reversible if compiler feedback or later requirements expose a weakness.

### Source checkpoint

- Task 4 implementation has not started; no Task 4 behavior has been validated.
- The repository is on `master` at commit `3b25b16`; `.github/workflows/ci.yml` has an uncommitted restoration of the format-and-Clippy CI job.
- The restored CI job mirrors the local formatting and strict Clippy checks. `./premerge.sh` passed all 41 tests, workflow diagnostics were clean, and local `actionlint` validation was unavailable because `actionlint` is not installed.

### Exact resume point

1. Refine `04-reader-writer-tasks.md` with the decisions above before starting implementation.
2. Change only the dispatch boundary first: make it return `Result<Option<Value>, _>`, while keeping `run_acp` sequential and calling the existing `write_frame` for `Some`.
3. Run the existing focused tests, `./premerge.sh`, and the unchanged handshake checker before introducing a channel; do not combine the dispatch refactor with task spawning.
4. After that behavior-preserving seam is green, decide the remaining Task 4 questions about Tokio features and writer-task error propagation just before they are needed.

## Session checkpoint — 2026-10-09 (Task 4 dispatch boundary)

### Current understanding

- Rust blocks return their final expression when it has no trailing semicolon; explicit `return` is reserved for leaving the function before that final expression.
- Dispatch can construct a protocol response without owning the output stream by returning `Result<Option<Value>, _>`.
- `Some(value)` represents a complete outbound protocol message, while Rust `Err` remains reserved for failures that prevent normal handling.
- Keeping `run_acp` sequential during this refactor isolated the ownership boundary change from the upcoming channel and task changes.

### Source checkpoint

- `dispatch` no longer accepts a writer and instead returns `Result<Option<Value>, _>`.
- `run_acp` still handles requests sequentially and calls the existing `write_frame` for each returned response.
- `./premerge.sh` passed all 41 tests, `cargo build --locked` exited 0, and the unchanged handshake checker exited 0 with report `/tmp/acp-task4-dispatch-refactor.json`.
- No channel or spawned-task code has been added yet.

### Exact resume point

1. Confirm from Tokio's documentation which crate feature enables `tokio::sync::mpsc`.
2. Add the channel and a dedicated writer loop without spawning tasks yet, preserving sequential coordination.
3. Defer the writer-task error propagation decision until task spawning makes it necessary.
4. The channel and writer-loop increment remains unimplemented and unverified.

## Session checkpoint — 2026-10-09 (Task 4 channel and writer loop)

### Current understanding

- Cargo recalculates enabled crate features from the current manifests, while `--locked` prevents commands from changing the package versions and dependency graph recorded in `Cargo.lock`.
- A bounded channel capacity of one follows the current sequential invariant: one complete response may wait while further production applies backpressure.
- `mpsc::Sender::send` transfers ownership of a complete `Value`; `Receiver::recv` returns `None` only after every sender is dropped and all queued values are drained.
- `tokio::try_join!` polls the reader and writer futures concurrently on the current task, allowing a response to be flushed while the reader still waits for input.
- A helper should remain private unless callers outside its module need it; `run_acp` remains the public transport boundary.

### Source checkpoint

- Tokio's `sync` feature is enabled in `Cargo.toml`; `Cargo.lock` did not change.
- `read_frames` owns the sole bounded-channel sender, and `write_frames` owns the receiver plus the only output-stream reference.
- `run_acp` uses `tokio::try_join!` to coordinate both loops without spawning tasks yet.
- ACP-focused tests passed 26 of 26, `./premerge.sh` passed all 41 tests, and `cargo build --locked` exited 0.
- The unchanged handshake checker exited 0, including `initialize_reply_arrives_before_eof`, with report `/tmp/acp-task4-channel.json`.

### Exact resume point

1. Decide how the coordinator should stop the remaining task and propagate the error when either spawned transport task fails while the other is blocked.
2. Then replace in-task concurrency with separately spawned reader and writer tasks, addressing the ownership and bounds required by `tokio::spawn` one compiler message at a time.
3. Add the required in-memory duplex tests for a response before client EOF and for draining queued responses after EOF.
4. Spawned-task behavior and the new duplex acceptance coverage remain unimplemented and unverified.

## Validation record

Add one structured entry after validation:

```text
Date: 2026-09-29
Task: 1 — Shared core
Commit or tree state: master; uncommitted changes in src/main.rs, src/terminal.rs, and guides/acp/progress.md
Commands: cargo fmt --all -- --check; cargo check --locked; cargo test --locked; if grep -nE '(^|[^e])println!' src/agent.rs; then exit 1; fi
Exit statuses: 0; 0; 0; 0
Tests run: 4
Tests passed: 4
Checker report path: none
```

```text
Date: 2026-10-05
Task: 2 — Interactive CLI
Commit or tree state: master, three commits ahead of origin/master; guides/acp/progress.md was modified when validation began
Commands: cargo fmt --all -- --check; cargo check --locked; cargo test --locked; cargo run --locked -- --help
Exit statuses: 0; 0; 0; 0
Tests run: 13
Tests passed: 13
Checker report path: none
```

```text
Date: 2026-10-09
Task: 3 — ACP initialize
Commit or tree state: initialize-acp at 2978a97; working tree clean before guides/acp/progress.md was updated
Commands: ./premerge.sh; cargo fmt --all -- --check; cargo check --locked; cargo test --locked; cargo build --locked; cargo run --locked -- --help; uv run --script guides/acp/check_handshake.py --report /tmp/acp-initialize-review.json --command /Users/peter/repos/hobgoblin/target/debug/hobgoblin --acp
Exit statuses: 0; 0; 0; 0; 0; 0; 0
Tests run: 41
Tests passed: 41
Checker cases passed: 7 of 7
Checker report path: /tmp/acp-initialize-review.json
```

Do not use prose explanations as completion evidence. Do not record credentials, complete model payloads, or unredacted sensitive paths.
