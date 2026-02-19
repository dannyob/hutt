#!/usr/bin/env python3
"""Test the hutt:// URL handler inside a Linux container.

Starts a mock Unix domain socket listener, invokes hutt-open with various
URLs, and verifies the received JSON commands match expectations.
"""

import json
import os
import socket
import subprocess
import sys
import threading
import time

SOCK_PATH = f"/tmp/hutt-{os.getuid()}.sock"

TESTS = [
    (
        "hutt://message/abc123@example.com",
        {"type": "Open", "kind": "Message", "id": "abc123@example.com"},
    ),
    (
        "hutt://thread/CAAkY2scdG_2W8@mail.gmail.com",
        {"type": "Open", "kind": "Thread", "id": "CAAkY2scdG_2W8@mail.gmail.com"},
    ),
    (
        "hutt://search/from%3Aalice",
        {"type": "Open", "kind": "Search", "query": "from:alice"},
    ),
    (
        "hutt://compose?to=bob%40example.com&subject=Hello%20World",
        {
            "type": "Open",
            "kind": "Compose",
            "to": "bob@example.com",
            "subject": "Hello World",
        },
    ),
]

passed = 0
failed = 0


def listen_once(server_sock, timeout=5):
    """Accept one connection and return the received data."""
    server_sock.settimeout(timeout)
    conn, _ = server_sock.accept()
    data = b""
    while True:
        chunk = conn.recv(4096)
        if not chunk:
            break
        data += chunk
    conn.close()
    return data


for url, expected in TESTS:
    test_name = url.split("://")[1].split("/")[0]

    # Clean up stale socket
    if os.path.exists(SOCK_PATH):
        os.unlink(SOCK_PATH)

    # Create the listener socket
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(SOCK_PATH)
    server.listen(1)

    # Start listener in a thread
    result = [None]

    def _listen(srv=server, res=result):
        try:
            res[0] = listen_once(srv)
        except socket.timeout:
            res[0] = None

    t = threading.Thread(target=_listen)
    t.start()

    # Give the listener a moment to be ready
    time.sleep(0.1)

    # Invoke the handler script directly (no xdg-open needed to test the
    # IPC logic — xdg-open just dispatches to hutt-open anyway)
    proc = subprocess.run(
        ["/usr/local/bin/hutt-open", url],
        capture_output=True,
        text=True,
        timeout=10,
    )

    t.join(timeout=10)
    server.close()

    if os.path.exists(SOCK_PATH):
        os.unlink(SOCK_PATH)

    if proc.returncode != 0:
        print(f"  FAIL [{test_name}]: handler exited {proc.returncode}")
        print(f"    stderr: {proc.stderr.strip()}")
        failed += 1
        continue

    if result[0] is None:
        print(f"  FAIL [{test_name}]: no data received on socket")
        failed += 1
        continue

    try:
        received = json.loads(result[0])
    except json.JSONDecodeError as e:
        print(f"  FAIL [{test_name}]: invalid JSON: {e}")
        print(f"    raw: {result[0]!r}")
        failed += 1
        continue

    if received == expected:
        print(f"  PASS [{test_name}]")
        passed += 1
    else:
        print(f"  FAIL [{test_name}]:")
        print(f"    expected: {json.dumps(expected)}")
        print(f"    received: {json.dumps(received)}")
        failed += 1


# --- Also test the .desktop file is correctly registered ----------------------

print()
print("Checking .desktop file registration...")

# Register for current user
subprocess.run(
    ["xdg-mime", "default", "hutt-opener.desktop", "x-scheme-handler/hutt"],
    capture_output=True,
)

proc = subprocess.run(
    ["xdg-mime", "query", "default", "x-scheme-handler/hutt"],
    capture_output=True,
    text=True,
)

desktop_file = proc.stdout.strip()
if desktop_file == "hutt-opener.desktop":
    print("  PASS [xdg-mime registration]")
    passed += 1
else:
    print(f"  FAIL [xdg-mime registration]: got '{desktop_file}', expected 'hutt-opener.desktop'")
    failed += 1


# --- Summary ------------------------------------------------------------------

print()
print(f"Results: {passed} passed, {failed} failed")
if failed > 0:
    sys.exit(1)
