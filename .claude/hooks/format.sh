#!/usr/bin/env bash
# PostToolUse hook: format the file an agent just edited, so agents never
# spend turns on style. Reads the hook payload (JSON) on stdin.
set -uo pipefail
file=$(python3 -c 'import json,sys; print(json.load(sys.stdin).get("tool_input",{}).get("file_path",""))' 2>/dev/null)
[ -n "$file" ] && [ -f "$file" ] || exit 0
case "$file" in
  *.rs) rustfmt --edition 2024 "$file" >/dev/null 2>&1 ;;
  *.ts|*.js|*.json|*.svelte|*.css)
    (cd "$CLAUDE_PROJECT_DIR" && pnpm exec biome format --write --no-errors-on-unmatched "$file" >/dev/null 2>&1) ;;
esac
exit 0
