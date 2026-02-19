#!/bin/bash
# hutt-open.sh — macOS URL handler for hutt:// URLs
#
# Receives a hutt:// URL (as the first argument from the .app bundle's
# CFBundleExecutable) and forwards it as a JSON IPC command to the running
# hutt instance via its Unix domain socket.
#
# The IPC protocol matches src/links.rs: IpcCommand is tagged by "type",
# and the Open variant embeds a HuttUrlSerde tagged by "kind".
#
# Socket location (same logic as Rust's socket_path()):
#   1. $XDG_RUNTIME_DIR/hutt.sock
#   2. /tmp/hutt-<uid>.sock

set -euo pipefail

URL="${1:-}"
if [ -z "$URL" ]; then
    exit 0
fi

# --- Locate the IPC socket ---------------------------------------------------

SOCK=""
if [ -n "${XDG_RUNTIME_DIR:-}" ] && [ -S "$XDG_RUNTIME_DIR/hutt.sock" ]; then
    SOCK="$XDG_RUNTIME_DIR/hutt.sock"
elif [ -S "/tmp/hutt-$(id -u).sock" ]; then
    SOCK="/tmp/hutt-$(id -u).sock"
fi

if [ -z "$SOCK" ]; then
    osascript -e 'display notification "No running hutt instance found." with title "Hutt"' 2>/dev/null || true
    exit 1
fi

# --- Parse the hutt:// URL into a JSON IPC command ----------------------------

# Strip the scheme prefix
REST="${URL#hutt://}"

build_json() {
    # Use python3 (always present on macOS) for safe JSON construction
    python3 -c "
import json, sys

rest = sys.argv[1]

if rest.startswith('message/'):
    mid = rest[len('message/'):]
    print(json.dumps({'type': 'Open', 'kind': 'Message', 'id': mid}))
elif rest.startswith('thread/'):
    tid = rest[len('thread/'):]
    print(json.dumps({'type': 'Open', 'kind': 'Thread', 'id': tid}))
elif rest.startswith('search/'):
    from urllib.parse import unquote
    query = unquote(rest[len('search/'):])
    print(json.dumps({'type': 'Open', 'kind': 'Search', 'query': query}))
elif rest.startswith('compose?'):
    from urllib.parse import parse_qs, unquote
    qs = rest[len('compose?'):]
    params = parse_qs(qs)
    to = params.get('to', [''])[0]
    subject = params.get('subject', [''])[0]
    print(json.dumps({'type': 'Open', 'kind': 'Compose', 'to': to, 'subject': subject}))
else:
    # Best-effort: treat the whole thing as a message ID
    print(json.dumps({'type': 'Open', 'kind': 'Message', 'id': rest}))
" "$1"
}

JSON=$(build_json "$REST")

# --- Send to the socket ------------------------------------------------------

if command -v socat &>/dev/null; then
    printf '%s' "$JSON" | socat - UNIX-CONNECT:"$SOCK"
elif command -v nc &>/dev/null && nc -h 2>&1 | grep -q '\-U'; then
    # BSD nc (macOS) supports -U for Unix domain sockets
    printf '%s' "$JSON" | nc -U "$SOCK"
else
    # Fallback: python3
    python3 -c "
import socket, sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(sys.argv[1])
s.sendall(sys.argv[2].encode())
s.close()
" "$SOCK" "$JSON"
fi
