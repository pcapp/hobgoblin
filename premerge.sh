#!/usr/bin/env bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D
cargo test --locked
