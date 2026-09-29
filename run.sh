#!/usr/bin/env bash
# To inspect model traffic, route it through a local proxy:
#   PROXY=http://127.0.0.1:8888 ./run.sh
if [[ -n "${PROXY:-}" ]]; then
  export HTTP_PROXY="$PROXY" HTTPS_PROXY="$PROXY"
fi
cargo build
RUST_LOG=hobgoblin=debug,warn ./target/debug/hobgoblin -p "Echo 'Hello, world!' to the console." 2> trace.jsonl
