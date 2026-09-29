#!/usr/bin/env bash
# Read Zed agent threads for this repository.
#
# Zed stores agent threads in a SQLite table whose `data` column is
# zstd-compressed JSON. This script lists the threads for this repo and
# renders one as Markdown: user turns, agent text, and one line per tool call.
# Thinking blocks are omitted.
#
# Usage:
#   scripts/zed-threads.sh              # list threads, newest first
#   scripts/zed-threads.sh <id-prefix>  # print one thread as Markdown
#
# Requires: sqlite3, zstd, jq.
set -euo pipefail

db="$HOME/Library/Application Support/Zed/threads/threads.db"
repo="$(cd "$(dirname "$0")/.." && pwd)"

if [[ $# -eq 0 ]]; then
  sqlite3 -separator '  ' "$db" \
    "select substr(id, 1, 8), substr(updated_at, 1, 16), summary
     from threads where folder_paths like '%$repo%'
     order by updated_at desc"
  exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

sqlite3 "$db" \
  "select writefile('$tmp/thread.zst', data) from threads
   where id like '$1%' and folder_paths like '%$repo%' limit 1" >/dev/null
[[ -s "$tmp/thread.zst" ]] || { echo "No thread matching '$1' for $repo" >&2; exit 1; }

zstd -dq "$tmp/thread.zst" -o "$tmp/thread.json"

jq -r '
  "# " + .title + " (" + .updated_at + ")",
  (.messages[] |
    if .User then
      "\n## USER\n" + ([.User.content[] |
        if .Text then .Text
        elif .Mention then "[@" + (.Mention.uri | (.File?.abs_path // tostring)) + "]"
        else "" end] | join(""))
    elif .Agent then
      [.Agent.content[] |
        if .Text then "\n## AGENT\n" + .Text
        elif .ToolUse then "  [tool " + .ToolUse.name + "] " + ((.ToolUse.input | tostring)[0:160])
        else empty end] | join("\n")
    else empty end)
' "$tmp/thread.json"
