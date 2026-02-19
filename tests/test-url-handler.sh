#!/usr/bin/env bash
# test-url-handler.sh — test the Linux/freedesktop URL handler in a container
#
# Uses podman to run a Fedora container with:
#   1. A mock IPC socket listener (python3)
#   2. The hutt-open.sh handler script
#   3. xdg-open triggering the handler via the .desktop file
#
# Usage:
#   ./tests/test-url-handler.sh
#
# Requires: podman

set -euo pipefail
cd "$(dirname "$0")/.."

CONTAINER_NAME="hutt-url-test-$$"
IMAGE_NAME="hutt-url-test"

# ---- Build the test container ------------------------------------------------

echo "==> Building test container..."
podman build -t "$IMAGE_NAME" -f - . <<'CONTAINERFILE'
FROM registry.fedoraproject.org/fedora:41

RUN dnf install -y python3 xdg-utils dbus-daemon && dnf clean all

# Create a test user
RUN useradd -m testuser

# Copy handler files
COPY macos/hutt-opener/Contents/MacOS/hutt-open.sh /usr/local/bin/hutt-open
RUN chmod +x /usr/local/bin/hutt-open

COPY linux/hutt-opener.desktop /usr/share/applications/hutt-opener.desktop

# Register the URL scheme system-wide
RUN mkdir -p /usr/share/applications && \
    update-desktop-database /usr/share/applications 2>/dev/null || true

# Test script that runs inside the container
COPY tests/container-url-test.py /opt/container-url-test.py

USER testuser
WORKDIR /home/testuser
CONTAINERFILE

# ---- Run the tests -----------------------------------------------------------

echo "==> Running URL handler tests..."
podman run --rm --name "$CONTAINER_NAME" "$IMAGE_NAME" \
    python3 /opt/container-url-test.py

echo "==> All tests passed."
