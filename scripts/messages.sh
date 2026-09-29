#!/usr/bin/env bash
# Print the request/response payloads logged in trace.jsonl (or the file given).
# -L adds this script's directory, where jq_defs.jq lives, to jq's module path.
dir="$(cd "$(dirname "$0")" && pwd)"
jq -L"$dir" 'include "jq_defs"; payload' "${1:-$dir/../trace.jsonl}"
