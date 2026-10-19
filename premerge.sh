#!/usr/bin/env bash
set -euo pipefail

cargo fmt --all -- --check
./guides/acp/acp_test_suite/premerge.sh
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
