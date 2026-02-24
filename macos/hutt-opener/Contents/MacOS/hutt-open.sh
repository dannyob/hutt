#!/bin/bash
# hutt-open.sh — macOS URL handler for mid:, message:, and hutt: URIs
#
# Receives a URI (as the first argument from the .app bundle's
# CFBundleExecutable) and forwards it to the running hutt instance
# via `hutt remote open-url`.
#
# Supports: mid:, message:, mailto:, hutt: schemes.

set -euo pipefail

URL="${1:-}"
if [ -z "$URL" ]; then
    exit 0
fi

# Find the hutt binary
HUTT="${HUTT_BIN:-hutt}"
if ! command -v "$HUTT" &>/dev/null; then
    HUTT="$HOME/.local/bin/hutt"
fi

if ! command -v "$HUTT" &>/dev/null; then
    osascript -e 'display notification "hutt binary not found." with title "Hutt"' 2>/dev/null || true
    exit 1
fi

# Delegate to hutt remote, which handles all URI parsing
"$HUTT" r open-url "$URL" 2>/dev/null || {
    osascript -e 'display notification "No running hutt instance found." with title "Hutt"' 2>/dev/null || true
    exit 1
}
