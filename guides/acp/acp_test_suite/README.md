# ACP test suite

This project drives the real Hobgoblin executable over newline-delimited JSON-RPC and records black-box ACP acceptance results.

From the repository root:

```sh
cargo build --locked
uv run --project guides/acp/acp_test_suite \
  guides/acp/acp_test_suite/test_handshake.py \
  --report /tmp/acp-test-suite.json \
  --executable target/debug/hobgoblin --acp
```

`--executable` must be the final test-suite option because all remaining arguments are passed to the executable. Relative executable paths are resolved before each test starts in an isolated temporary working directory.

Run the Python quality checks directly with:

```sh
./guides/acp/acp_test_suite/premerge.sh
```

The repository-level `./premerge.sh` runs these checks as well.
