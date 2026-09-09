# Lab 06 — Close the baseline gaps and test Zed honestly

**User-visible milestone:** Zed can send text plus a resource link, use a controlled stdio MCP tool through the agent, and recover from a cancelled turn. Unsupported features fail clearly. The original CLI still runs independently.

**Status:** generated coursework, not a compatibility certification. Peter types the implementation and gathers actual evidence; an echo demo is not a conformant ACP agent.

[Course](README.md) · [Progress](progress.md) · [Previous: cancellation](05-cancellation.md)

## Prerequisites and version scope

- Pass Labs 03–05, including provider mocks, approval, workspace isolation, cancellation races, and cleanup.
- Retain the standalone handshake, scripted echo tests, and integrated `--acp` startup without a model key from stage 02.
- Teaching protocol target: stable ACP **v1**, learner-owned envelopes/DTOs, router, request correlation, serialized writer queue, and supervised turn tasks. Serde/serde_json and Tokio are allowed; **no ACP agent or schema SDK dependencies**.
- ACP and MCP have different version negotiations and different roles: Zed is the ACP client; your agent is the MCP client of each configured server.
- Read [API baseline](file:///Users/peter/knowledge-bundles/harness-engineering/protocols/acp-api-map.md), [learner-owned Rust protocol](file:///Users/peter/knowledge-bundles/harness-engineering/references/acp-rust-protocol.md), [safety/cancellation](file:///Users/peter/knowledge-bundles/harness-engineering/concepts/acp-tools-and-cancellation.md), and [evidence policy](file:///Users/peter/knowledge-bundles/harness-engineering/workflows/acp-learning.md).

## Why these are not optional polish

[v1 initialization](https://agentclientprotocol.com/protocol/v1/initialization) requires baseline text and resource-link prompts. Image/audio/embedded resources are separate optional capabilities. [v1 session setup](https://agentclientprotocol.com/protocol/v1/session-setup#mcp-servers) requires stdio MCP support; HTTP/SSE MCP are optional. Empty `mcpServers` in early labs postponed implementation, not the requirement.

Use your own Serde content DTOs at ingress, then domain prompt parts at the provider boundary. Keep URI identity separate from retrieved text; preserving a resource link does not authorize arbitrary filesystem or network access. Use an established URI parser such as `url` for conversion rather than stripping `file://` manually; reuse Lab 04's path and permission policy.

For MCP, keep a dedicated client boundary in `src/acp/mcp.rs`. **MCP library choice is separate and optional, not silently decided by this course:** choose a learner-owned implementation of the needed MCP subset, or explicitly approve a dedicated MCP client library after a small dependency/build spike. A library choice must not bring an ACP agent/schema SDK into the dependency graph or supply your ACP DTOs. No exact MCP crate/version is selected here. If choosing a library, inspect its client, transport, process, and fixture features and verify its resolved dependencies yourself.

Whichever implementation you choose, use the canonical [MCP specification](https://modelcontextprotocol.io/specification/2025-06-18) for a documented version: verify [lifecycle](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle), [stdio transport](https://modelcontextprotocol.io/specification/2025-06-18/basic/transports), [tools](https://modelcontextprotocol.io/specification/2025-06-18/server/tools), and [cancellation](https://modelcontextprotocol.io/specification/2025-06-18/basic/utilities/cancellation). This is a fixture version target, not a claim that it is the newest MCP version. Stable stdio MCP does not require experimental MCP-over-ACP bridging. No additional crate was installed during guide authoring.

## Before editing: audit the remaining boundary

| Current source / learner-created target | Audit question |
|---|---|
| `src/main.rs:11–14`, `Args.prompt`; `:28–44`, provider construction | Did stage 02 really remove the prompt/key requirement for protocol startup while preserving CLI? |
| `src/agent.rs:12–23`, `messages` and provider request | Can a resource link survive conversion without being silently dropped or misrepresented as fetched text? |
| `src/wire.rs::ChatResponse`, `Message`, `ToolCall` | Are provider DTOs still separate from ACP content and MCP tool result types? |
| `src/tools.rs::specs`, `execute_tool_call` | Can session-local MCP tools coexist with Read/Write/Bash without name collisions or bypassing policy? |
| `src/acp/mod.rs` (or existing `src/acp.rs`) | Are initialized state, capabilities, unsupported methods, session setup, and prompt finalization explicit? |

```sh
cargo test --locked acp_stage03_
cargo test --locked acp_stage04_
cargo test --locked acp_stage05_
cargo test --locked -- --list
```

Inspect actual test discovery before trusting these filters. Line references above describe the original source; locate symbols again after your earlier changes.

## Incremental learner actions

### 1. Write the supported-profile matrix first

In the review evidence you will later record in `guides/acp/progress.md`, distinguish **required**, **implemented/tested**, **not implemented**, and **not tested**. Start with this target, not a claim of current support:

| Surface | Target policy |
|---|---|
| initialize/new/prompt/update/cancel, text/resource link, stdio MCP | Required baseline; must be implemented and tested before declaring this milestone. |
| Client read/write/terminal | Use only when advertised; obey approval/cwd policy; no silent local fallback. |
| Image/audio/embedded context, load/resume/list/close, richer UI | Do not advertise until implemented; negative tests must show no false success. |
| HTTP/SSE MCP, remote ACP transports | Not part of this local-stdio acceptance profile. |

**Rationale:** capability advertising is a promise, not a wish list. An MCP failure must not produce a silently empty successful session.

### 2. Finish resource-link handling end to end

Create `src/acp/content.rs:1` for typed prompt conversion; extend proposed `src/session.rs` domain input if needed. Match text and resource links in your `session/prompt` parameter DTO; preserve part order, name, URI, and supplied metadata. Map references into an explicit provider-visible representation, not an invented file body.

This **illustrative ACP input**, grounded in canonical [v1 resource links](https://agentclientprotocol.com/protocol/v1/content#resource-link) and [prompt requests](https://agentclientprotocol.com/protocol/v1/prompt-turn), is not a captured request. Its path is a fixture label; tests substitute their temporary absolute URI:

```json
{"jsonrpc":"2.0","id":30,"method":"session/prompt","params":{"sessionId":"s1","prompt":[{"type":"text","text":"Read this reference if permitted"},{"type":"resource_link","name":"notes.txt","uri":"file:///tmp/acp-lab/notes.txt","mimeType":"text/plain"}]}}
```

Implement supported file-link resolution through the client Read route when requested and permitted; preserve unsaved-buffer semantics. A blocked, unavailable, or unsupported URI remains an explicit reference with a clear limitation, never fabricated content. Do not fetch `https:`/other schemes merely because they arrived in a prompt. Unsupported optional content must fail before provider work, not disappear silently.

**Rationale:** accepting the baseline link type, preserving its meaning, and safely resolving a permitted reference are separate assertions to test.

### 3. Build a small, controlled MCP fixture

Create proposed `src/bin/acp_mcp_fixture.rs:1` yourself as a test-only-purpose executable, then `tests/acp_compatibility.rs:1` for black-box tests. The fixture should speak actual MCP over stdio using a strictly bounded learner-owned protocol implementation or your separately approved MCP library. Do not install or launch arbitrary third-party MCP servers.

Define its exact contract: negotiate one documented supported MCP version, complete initialize/initialized, answer paginated `tools/list`, and expose `echo_label` with one string argument `label`. A call with `cedar` returns text `fixture:cedar`. A second tool `wait_for_release` announces started on a test-control channel and waits, for cancellation testing. Add controlled failure modes for initialization failure and tool-level error.

The fixture never reads personal files or the network, never accepts arbitrary commands, and sends diagnostics to stderr. Give each instance a harmless session marker through an explicit environment entry; tests verify cwd/marker delivery without dumping environment variables.

```sh
cargo build --locked --bin acp_mcp_fixture
```

**Rationale:** a real child protocol peer proves more than asserting that an MCP server config was stored. A fixture is still not proof of compatibility with arbitrary servers.

### 4. Connect session-owned MCP clients before claiming readiness

In `src/acp/mcp.rs:1`, implement `SessionMcpClients`; in the `session/new` route in `src/acp/mod.rs`, reserve a provisional session, move setup and the original request ID/response obligation into supervised async work, and resume routing promptly. Validate configuration, connect and initialize each stdio server, collect tools, then enqueue the correlated response containing the session ID. Keep slow startup outside the reader/router too. Each MCP child has its own private transport, request-ID allocator, bounded pending map, and teardown ownership (or the equivalent guaranteed by an approved MCP library); never mix those IDs/replies with the ACP connection's map.

This **design sketch** intentionally uses domain labels rather than library method signatures:

```text
McpServerStdio(command, args, env) → validated child transport
→ MCP initialize/version negotiation → initialized → tools/list (all pages)
→ session-local registry(server identity, tool name, input schema)
→ provider tool request → policy/approval → tools/call → typed tool result
session teardown/disconnect → cancel service → close pipes → kill/reap if needed
```

Use the executable/argv exactly as validated, without shell interpolation; set session cwd explicitly. Pass configured env entries with a reviewed inheritance policy, keep provider secrets out of child env by default, and pipe MCP stdout privately—it must never enter ACP stdout. Reject unsupported transports explicitly without network attempts.

For the course, fail session setup clearly if a configured server cannot initialize, and clean up previously started servers; do not return a successful session that silently discarded them. Bound startup, requests, pagination, output, and shutdown. Keep a connection-level teardown path even if optional `session/close` is not advertised.

**Rationale:** session ownership includes subprocess lifetime, startup failures, and servers still starting when the ACP peer disconnects.

### 5. Route MCP tools through the existing safety boundary

Extend `src/tools.rs::specs` into a session-aware catalog and route invocations through the Lab 04 executor boundary. Namespace MCP tool names reversibly (or map opaque provider-safe names) so two servers named tools alike cannot collide with each other or built-ins.

Preserve MCP tool-level failure separately from transport failure; map text results and unsupported rich content honestly. Server descriptions/annotations are untrusted input, not authority to bypass approval. Ask approval for unknown/sensitive MCP effects under the course policy; for the controlled read-only echo fixture, an explicit test policy may allow it.

Propagate Lab 05's cancellation into in-flight MCP work using the negotiated MCP version's cancellation semantics; implement them yourself or verify the separately chosen library exposes them. For the documented fixture version, `notifications/cancelled` refers to an outstanding MCP request; clients must not cancel MCP `initialize` this way. Bound failed/stalled initialization through owned-child teardown instead. Stop new calls, settle results, and retain or restart the session service deliberately. Keep MCP request cancellation distinct from ACP `session/cancel`; a cancelled local waiter is not proof of remote termination. On disconnect or failed setup, stop/reap all owned fixture children. **Rationale:** “external tool” cannot mean “outside our cancellation and approval policy.”

### 6. Exercise transport and failure boundaries before Zed

In `tests/acp_compatibility.rs`, spawn the actual `codecrafters-claude-code` binary with `--acp`; use `CARGO_BIN_EXE_codecrafters-claude-code` rather than assuming a build-directory path. Put test-only support in `tests/support/mod.rs`. Unit-test `#[cfg(test)]` providers are not available in a normally built executable: do not assume Lab 03's fake can be selected in production.

For black-box provider prompts, implement a loopback HTTP scripted server in `tests/support/provider_http.rs`: bind an OS-assigned localhost port, capture completion requests, and return bounded OpenAI-compatible responses from the same fixture queue as Lab 03. Set only that child's existing `OPENROUTER_BASE_URL` to its loopback `/v1` endpoint and supply a literal dummy API key; prevent external egress and reject unplanned requests. No production test mode or real provider key is needed. Use separate no-key children for startup/error cases.

The harness owns stdin/stdout/stderr and child cleanup, writes compact JSON lines, collects frames by request ID, and handles reverse requests while prompts are outstanding. Do not block waiting for prompt completion before answering permissions. Test no-key startup from a temporary cwd so repo `.env` loading cannot accidentally supply credentials; remove provider keys from the child environment explicitly.

**Rationale:** black-box framing and no-key startup checks catch regressions that unit tests calling route functions directly cannot see. Drive the actual reader/router and writer queue, including reverse replies while turns remain pending.

## Named deterministic tests: Arrange / Act / Assert

Put the following in `tests/acp_compatibility.rs`; use the scripted provider from Lab 03 and controlled fixture, never a live model. These are test specifications to type, not tests already implemented.

- **`acp_stage06_resource_link_preserved_and_resolved`** — Arrange text/link/text parts and a client buffer differing from disk. Act with a prompt and scripted Read. Assert order/URI/name retained in provider input and resolved content comes from the client; no unapproved network fetch.
- **`acp_stage06_optional_content_rejected`** — Arrange no image/audio/embedded-context capabilities. Act with each corresponding content kind. Assert a clear error before provider calls; baseline text/link still works afterward.
- **`acp_stage06_mcp_roundtrip`** — Arrange a configured fixture with cwd/env marker and paginated tool list. Act with new session and scripted `echo_label(cedar)`. Assert MCP initialization, all list pages consumed, actual tool invocation, `fixture:cedar` in model/UI result, and no MCP frames on ACP stdout.
- **`acp_stage06_mcp_failures_and_isolation`** — Arrange duplicate tool names across servers/sessions and separate startup/tool-error cases. Act through setup/call. Assert collision-free routing, no cross-session markers, clear startup failure with cleanup, and failed—not successful—tool results.
- **`acp_stage06_mcp_cancel_and_disconnect`** — Arrange `wait_for_release` started. Act with cancel, then a new turn; separately disconnect during setup/call. Assert one cancelled prompt, a usable next turn, and all owned fixture children reaped on teardown.
- **`acp_stage06_no_key_startup_and_cli`** — Arrange separate clean no-key and dummy-key loopback-provider children in a temporary cwd. Act with no-key `--acp` initialize/new, then no-key provider prompt; separately run CLI help and `-p` against the loopback fixture. Assert no startup key requirement for ACP, an explicit prompt-time error for the missing key, preserved CLI answer/exit semantics, and zero real-provider traffic.
- **`acp_stage06_version_and_request_errors`** — Arrange fresh processes for requested versions 1/2, pre-initialize new, unknown method, unknown session, and invalid prompt params. Act once per case. Assert version 1 selected (not echoed 2), no pre-init session, correlated errors for invalid requests, no state mutation, and no panic.
- **`acp_stage06_framing_and_capabilities`** — Arrange split JSON writes, multiple frames in one write, malformed JSON, incomplete EOF, and absent/read-only/full client capabilities. Act through bounded probes. Assert complete frames correlate, errors/clean closure follow your documented framing policy and JSON-RPC rules, no hangs/raw stdout logs, and no forbidden client calls or local fallback.
- **`acp_stage06_bidirectional_ids_and_limits`** — Arrange equal inbound/outbound IDs, string versus numeric IDs, out-of-order and duplicate reverse replies, full pending/output queues, and writer failure. Act while a prompt is outstanding. Assert correct oneshot correlation, no response to a reply, no cross-turn authorization, bounded overload/teardown, and no orphan tasks or unresolved waiters hidden as success.

For framing, inspect canonical [ACP stdio](https://agentclientprotocol.com/protocol/v1/transports) and [JSON-RPC errors](https://www.jsonrpc.org/specification#error_object), then test the policy you own. For a complete newline-delimited malformed JSON frame, emit parse error `-32700` with null ID; for an invalid request envelope, use `-32600` with the appropriate ID rule. Valid unknown methods get `-32601`; invalid method params get `-32602`, correlated to the original ID. Unknown notifications receive no response; malformed/unknown replies must not start an error-response loop. Preserve string/numeric ID distinctions and detect success by field presence, including `result: null`. Test unknown extension fields without granting unsupported capabilities.

Preserve Lab 01's local frame limit of **64 KiB including the terminating newline**, enforced before unbounded allocation. Oversized input and unterminated EOF report to stderr and close nonzero; EOF with no buffered fragment is clean. Document bounded teardown for invalid UTF-8 too; never execute a partial frame. ACP stdio carries individual messages, not a batch array. Test boundary-size frames and your configured writer-queue/pending-request limits; these limits and deadlines are local policy, not protocol constants. Stop accepting work and settle/join owned tasks on EOF or write failure; do not claim a response was delivered on a dead pipe. Include an open-stdin flush test rather than relying on the Lab 01 checker to prove that property.

```sh
cargo test --locked --test acp_compatibility acp_stage06_ -- --list
cargo test --locked --test acp_compatibility acp_stage06_ -- --nocapture
cargo test --locked
cargo check --locked
cargo build --locked --bin codecrafters-claude-code
```

Check every named test is discovered and executes. Use readiness gates plus bounded timeouts; kill/reap harness children on assertion failure as well as success. An ignored fixture test leaves the baseline MCP gate open.

## Final Zed acceptance: manual, recorded, and bounded

1. Record the installed Zed version, OS, built binary path, git revision/diff, ACP capabilities, and negotiated MCP fixture version. Do not reuse a historical version from the course as if measured today.
2. Use [Zed Custom Agents](https://zed.dev/docs/ai/external-agents#custom-agents): Agent Settings → External Agents → Add Agent → Add Custom Agent. Configure absolute `/Users/peter/repos/codecrafters-claude-code-rust/target/debug/codecrafters-claude-code` and arguments matching your implemented `--acp` mode; keep credentials out of versioned settings/evidence.
3. Open a disposable project. Start two external-agent threads; verify independent histories and cwd. Begin with a deterministic diagnostic provider; explicitly approve any live model cost separately.
4. Send text and attach/reference a scratch file. Check the ACP log shows a resource link for this baseline test, not merely embedded text; if Zed sends another kind, retain the harness proof and mark this UI path untested.
5. Deny a write and read back unchanged contents; approve another and read back the change. Read an unsaved buffer; run a controlled failing command; confirm failed status and terminal release.
6. Configure only the controlled stdio MCP fixture. Inspect `session/new.mcpServers`, discover/invoke `echo_label`, and verify its actual result. If Zed does not forward it, investigate configuration rather than silently skipping MCP.
7. Cancel during provider work, permission, terminal execution, and fixture wait. Confirm cleanup, one final response, no late updates, and a working subsequent prompt.
8. Use Command Palette → `dev: open acp logs`. Retain redacted request/update/response ordering and actual test output; inspect for secrets before sharing. Run the CLI regression again independently.

## Acceptance gate and teach-back

- [ ] All deterministic named tests and prior labs pass; stdout is protocol-only; session/process cleanup is verified.
- [ ] Baseline text/resource links and actual stdio MCP initialize/list/call/teardown are demonstrated, not merely advertised.
- [ ] Missing capabilities and unsupported features fail without bypassing policy; CLI and no-key ACP startup are retained.
- [ ] Zed evidence records what actually ran, including version, capabilities, fixture scope, remaining untested cases, and any blockers. Claim only this tested profile, never every ACP client/server.

Explain why “no optional capabilities” still requires resource links and stdio MCP; distinguish ACP vs MCP request IDs/roles/versions; show how MCP tool output becomes a provider result without becoming ACP stdout. Identify one passing test that cannot prove general compatibility.

Record learner explanations and actual results in [progress](progress.md) during review; generated content does not promote mastery. **Snippet status:** the resource-link JSON follows canonical v1 content/prompt fields; it is an illustration, not captured traffic. Historical SDK deserialization checks are superseded, not validation of your DTOs. Control flow is a design sketch. No MCP dependency choice, fixture, revised application integration, or Zed acceptance was built/run during this documentation revision.
