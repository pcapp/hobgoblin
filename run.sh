#!/usr/bin/env bash
export HTTP_PROXY=http://127.0.0.1:8888
export HTTPS_PROXY=http://127.0.0.1:8888
cargo build
RUST_LOG=hobgoblin=debug,warn ./target/debug/hobgoblin -p "Echo 'Hello, world!' to the console." 2> trace.jsonl
