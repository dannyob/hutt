#!/usr/bin/env python3
"""Extract split inbox definitions from a local Superhuman installation.

Usage:
    # List all splits from all accounts
    ./scripts/superhuman-import.py

    # Export as hutt splits TOML for a specific account
    ./scripts/superhuman-import.py --account user@example.com --hutt

    # Custom Superhuman data directory
    ./scripts/superhuman-import.py --dir ~/Library/Application\\ Support/Superhuman

    # JSON output
    ./scripts/superhuman-import.py --json
"""

import argparse
import json
import os
import re
import sqlite3
import struct
import sys
from pathlib import Path


def find_superhuman_dir():
    """Locate the Superhuman Electron app data directory."""
    candidates = [
        Path.home() / "Library" / "Application Support" / "Superhuman",  # macOS
        Path.home() / ".config" / "Superhuman",  # Linux
        Path.home() / "AppData" / "Roaming" / "Superhuman",  # Windows
    ]
    for p in candidates:
        if p.is_dir():
            return p
    return None


def find_account_dbs(superhuman_dir):
    """Find SQLite databases in Superhuman's File System storage.

    Each account gets a blob file whose first 4096 bytes contain the
    account email, followed by a SQLite database.
    """
    fs_dir = superhuman_dir / "File System" / "000" / "t" / "00"
    if not fs_dir.is_dir():
        return []

    accounts = []
    for f in sorted(fs_dir.iterdir()):
        if not f.is_file() or f.stat().st_size < 8192:
            continue
        with open(f, "rb") as fh:
            header = fh.read(4096)
        # Header is: /<email>.sqlite3\x00...
        # Extract the email address
        try:
            text = header.split(b"\x00", 1)[0].decode("utf-8", errors="replace")
        except Exception:
            continue
        if ".sqlite3" not in text:
            continue
        # Parse: /user@example.com.sqlite3
        email = text.lstrip("/").replace(".sqlite3", "")
        if "@" not in email:
            continue
        accounts.append((email, f))
    return accounts


def extract_settings(db_path):
    """Extract the settings JSON from a Superhuman SQLite database.

    The SQLite data starts at offset 4096 in the blob file.
    """
    tmp = "/tmp/_superhuman_extract.sqlite3"
    with open(db_path, "rb") as f:
        f.seek(4096)
        data = f.read()
    with open(tmp, "wb") as f:
        f.write(data)

    try:
        conn = sqlite3.connect(tmp)
        cursor = conn.execute("SELECT json FROM general WHERE key='settings'")
        row = cursor.fetchone()
        conn.close()
        if row:
            return json.loads(row[0])
    except Exception as e:
        print(f"  Warning: could not read database: {e}", file=sys.stderr)
    finally:
        try:
            os.unlink(tmp)
        except OSError:
            pass
    return None


def superhuman_query_to_mu(query):
    """Best-effort conversion of Superhuman split query to mu search syntax.

    Superhuman uses its own query language that's a subset of Gmail search
    with some custom additions. We translate what we can.

    Returns (mu_query, notes) where notes lists any conversion issues.
    """
    if not query or not query.strip():
        return ("", ["empty query — Superhuman built-in ML classifier, no mu equivalent"])

    notes = []
    q = query.strip()

    # Superhuman-specific predicates we can't translate
    if "is:shared" in q:
        return ("", ["'is:shared' is Superhuman-specific (shared threads feature)"])

    # is:starred → flag:flagged
    q = re.sub(r'\bis:starred\b', 'flag:flagged', q)
    if 'flag:flagged' in q:
        notes.append("is:starred → flag:flagged")

    # filename:ics → mime:text/calendar (approximate)
    if 'filename:ics' in q:
        q = re.sub(r'\bfilename:ics\b', 'mime:text/calendar', q)
        notes.append("filename:ics → mime:text/calendar (approximate)")

    # to:me → to:$EMAIL (needs account context, leave as-is with note)
    if 'to:me' in q:
        notes.append("'to:me' left as-is — replace with your email address")

    # Parenthesized groups: (A OR B) — mu supports this
    # OR → or (mu uses lowercase)
    q = re.sub(r'\bOR\b', 'or', q)

    # from: queries pass through directly (mu supports them)

    return (q, notes)


def format_hutt_toml(splits):
    """Format splits as hutt splits TOML."""
    lines = []
    for s in splits:
        if not s["mu_query"]:
            lines.append(f'# Skipped: {s["name"]} — {"; ".join(s["notes"])}')
            continue
        lines.append(f'[[splits]]')
        lines.append(f'name = "{s["name"]}"')
        lines.append(f'query = "{s["mu_query"]}"')
        if s["notes"]:
            lines.append(f'# Notes: {"; ".join(s["notes"])}')
        if s.get("disabled"):
            lines.append(f'# (was disabled in Superhuman)')
        lines.append("")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(
        description="Extract split inbox definitions from Superhuman"
    )
    parser.add_argument(
        "--dir",
        type=Path,
        help="Superhuman data directory (auto-detected if omitted)",
    )
    parser.add_argument(
        "--account",
        help="Filter to a specific account email address",
    )
    parser.add_argument(
        "--hutt",
        action="store_true",
        help="Output as hutt splits TOML format",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output as JSON",
    )
    parser.add_argument(
        "--include-disabled",
        action="store_true",
        help="Include disabled splits",
    )
    args = parser.parse_args()

    sh_dir = args.dir or find_superhuman_dir()
    if not sh_dir or not sh_dir.is_dir():
        print("Error: Superhuman data directory not found.", file=sys.stderr)
        print("Try: --dir ~/Library/Application\\ Support/Superhuman", file=sys.stderr)
        sys.exit(1)

    print(f"Superhuman data: {sh_dir}", file=sys.stderr)

    accounts = find_account_dbs(sh_dir)
    if not accounts:
        print("Error: No account databases found.", file=sys.stderr)
        sys.exit(1)

    print(f"Found {len(accounts)} account(s): {', '.join(e for e, _ in accounts)}", file=sys.stderr)

    all_results = {}
    for email, db_path in accounts:
        if args.account and email != args.account:
            continue

        settings = extract_settings(db_path)
        if not settings:
            print(f"  {email}: no settings found", file=sys.stderr)
            continue

        raw_splits = settings.get("splitInboxes", [])
        if not raw_splits:
            print(f"  {email}: no splits configured", file=sys.stderr)
            continue

        splits = []
        for s in raw_splits:
            matcher = s.get("matcher", {})
            name = matcher.get("name", "unnamed")
            sh_query = matcher.get("query", "")
            gmail_query = matcher.get("gmailQuery", "")
            disabled = s.get("isDisabled", False)
            split_type = s.get("type", "custom")

            if disabled and not args.include_disabled:
                continue

            mu_query, notes = superhuman_query_to_mu(sh_query)

            splits.append({
                "name": name,
                "superhuman_query": sh_query,
                "gmail_query": gmail_query,
                "mu_query": mu_query,
                "type": split_type,
                "disabled": disabled,
                "notes": notes,
            })

        all_results[email] = splits

    if not all_results:
        print("No splits found.", file=sys.stderr)
        sys.exit(1)

    # Output
    if args.json:
        print(json.dumps(all_results, indent=2))
    elif args.hutt:
        for email, splits in all_results.items():
            if len(all_results) > 1:
                print(f"# === {email} ===")
            print(format_hutt_toml(splits))
    else:
        # Human-readable table
        for email, splits in all_results.items():
            print(f"\n{'='*60}")
            print(f"  {email}")
            print(f"{'='*60}")
            for s in splits:
                status = "DISABLED" if s["disabled"] else "active"
                mu = s["mu_query"] or "(no mu equivalent)"
                print(f"\n  {s['name']} [{status}] (type: {s['type']})")
                print(f"    Superhuman: {s['superhuman_query'] or '(empty)'}")
                print(f"    Gmail:      {s['gmail_query'] or '(empty)'}")
                print(f"    mu:         {mu}")
                if s["notes"]:
                    for n in s["notes"]:
                        print(f"    ⚠ {n}")


if __name__ == "__main__":
    main()
