#!/usr/bin/env bash
# Repository content must be English; only locales/ may hold other languages.
set -euo pipefail
if rg --hidden -n '\p{Cyrillic}' --glob '!locales/**' --glob '!.git/**' --glob '!**/node_modules/**' --glob '!target/**' .; then
  echo "error: Cyrillic text found outside locales/ (see above)" >&2
  exit 1
fi
