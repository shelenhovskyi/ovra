#!/usr/bin/env bash
# Repository content and the UI are English only (no translations for now).
set -euo pipefail
if rg --hidden -n '\p{Cyrillic}' --glob '!.git/**' --glob '!**/node_modules/**' --glob '!target/**' .; then
  echo "error: Cyrillic text found (see above); repository content and the UI are English only" >&2
  exit 1
fi
