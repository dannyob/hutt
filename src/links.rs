//! Phase 3: Linkability — hutt:// URLs, clipboard, browser open, IPC socket.

use anyhow::{bail, Context, Result};
use arboard::Clipboard;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

// ---------------------------------------------------------------------------
// hutt:// URL scheme
// ---------------------------------------------------------------------------

/// Parsed representation of a `hutt://` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HuttUrl {
    Message(String),
    Thread(String),
    Search(String),
    Compose { to: String, subject: String },
}

/// Format a `hutt://message/<message-id>` URL.
pub fn format_message_url(message_id: &str) -> String {
    format!("hutt://message/{}", message_id)
}

/// Format a `hutt://thread/<message-id>` URL.
pub fn format_thread_url(message_id: &str) -> String {
    format!("hutt://thread/{}", message_id)
}

/// Format a `hutt://search/<url-encoded-query>` URL.
pub fn format_search_url(query: &str) -> String {
    let encoded = url_encode(query);
    format!("hutt://search/{}", encoded)
}

/// Format a `hutt://compose?to=<to>&subject=<subject>` URL.
pub fn format_compose_url(to: &str, subject: &str) -> String {
    format!(
        "hutt://compose?to={}&subject={}",
        url_encode(to),
        url_encode(subject),
    )
}

/// Parse a `hutt://` URL into a `HuttUrl`, returning `None` if it's not valid.
pub fn parse_hutt_url(url: &str) -> Option<HuttUrl> {
    let rest = url.strip_prefix("hutt://")?;

    if let Some(id) = rest.strip_prefix("message/") {
        if id.is_empty() {
            return None;
        }
        return Some(HuttUrl::Message(id.to_string()));
    }

    if let Some(id) = rest.strip_prefix("thread/") {
        if id.is_empty() {
            return None;
        }
        return Some(HuttUrl::Thread(id.to_string()));
    }

    if let Some(encoded) = rest.strip_prefix("search/") {
        let query = url_decode(encoded);
        if query.is_empty() {
            return None;
        }
        return Some(HuttUrl::Search(query));
    }

    if let Some(query_string) = rest.strip_prefix("compose?") {
        let params = parse_query_string(query_string);
        let to = params.get("to").cloned().unwrap_or_default();
        let subject = params.get("subject").cloned().unwrap_or_default();
        return Some(HuttUrl::Compose { to, subject });
    }

    None
}

// ---------------------------------------------------------------------------
// Clipboard
// ---------------------------------------------------------------------------

/// Copy text to the system clipboard.
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    let mut clipboard = Clipboard::new().context("failed to access clipboard")?;
    clipboard
        .set_text(text)
        .context("failed to copy to clipboard")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Open in browser
// ---------------------------------------------------------------------------

/// Write HTML bytes to a temp file and open it in the default browser.
pub fn open_html_in_browser(html: &[u8]) -> Result<()> {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("hutt-{}.html", std::process::id()));
    std::fs::write(&path, html)
        .with_context(|| format!("writing temp HTML to {}", path.display()))?;
    open_path(path.to_str().context("non-UTF-8 temp path")?)
}

/// Open a URL (or file path) in the default browser / handler.
pub fn open_url(url: &str) -> Result<()> {
    open_path(url)
}

fn open_path(target: &str) -> Result<()> {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };

    std::process::Command::new(cmd)
        .arg(target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("spawning {} {}", cmd, target))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// IPC socket
// ---------------------------------------------------------------------------

/// Commands that can be sent over the IPC socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum IpcCommand {
    Open(HuttUrlSerde),
    Navigate { folder: String },
    Quit,
}

/// Serde-friendly mirror of `HuttUrl` (the enum above uses untagged variants
/// which are tricky with serde, so we keep a dedicated transport type).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum HuttUrlSerde {
    Message { id: String },
    Thread { id: String },
    Search { query: String },
    Compose { to: String, subject: String },
}

impl From<HuttUrl> for HuttUrlSerde {
    fn from(u: HuttUrl) -> Self {
        match u {
            HuttUrl::Message(id) => HuttUrlSerde::Message { id },
            HuttUrl::Thread(id) => HuttUrlSerde::Thread { id },
            HuttUrl::Search(q) => HuttUrlSerde::Search { query: q },
            HuttUrl::Compose { to, subject } => HuttUrlSerde::Compose { to, subject },
        }
    }
}

impl From<HuttUrlSerde> for HuttUrl {
    fn from(s: HuttUrlSerde) -> Self {
        match s {
            HuttUrlSerde::Message { id } => HuttUrl::Message(id),
            HuttUrlSerde::Thread { id } => HuttUrl::Thread(id),
            HuttUrlSerde::Search { query } => HuttUrl::Search(query),
            HuttUrlSerde::Compose { to, subject } => HuttUrl::Compose { to, subject },
        }
    }
}

/// Determine the IPC socket path.
fn socket_path() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("hutt.sock")
    } else {
        let uid = unsafe { libc::getuid() };
        PathBuf::from(format!("/tmp/hutt-{}.sock", uid))
    }
}

/// Server-side IPC listener wrapping a tokio `UnixListener`.
pub struct IpcListener {
    listener: UnixListener,
    path: PathBuf,
}

impl IpcListener {
    /// Create and bind the Unix domain socket.  Removes a stale socket file
    /// if one already exists.
    pub fn bind() -> Result<Self> {
        let path = socket_path();
        // Remove stale socket if present.
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("removing stale socket {}", path.display()))?;
        }
        let listener =
            UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
        Ok(Self { listener, path })
    }

    /// Accept a single connection, read a JSON-encoded `IpcCommand`, and
    /// return it.
    pub async fn accept(&self) -> Result<IpcCommand> {
        let (mut stream, _addr) = self
            .listener
            .accept()
            .await
            .context("accepting IPC connection")?;

        let mut buf = Vec::with_capacity(4096);
        stream
            .read_to_end(&mut buf)
            .await
            .context("reading IPC command")?;

        let cmd: IpcCommand =
            serde_json::from_slice(&buf).context("deserializing IPC command")?;
        Ok(cmd)
    }
}

impl Drop for IpcListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Client side: connect to the running hutt instance and send a command.
pub async fn send_ipc_command(cmd: &IpcCommand) -> Result<()> {
    let path = socket_path();
    if !path.exists() {
        bail!(
            "no hutt instance running (socket {} not found)",
            path.display()
        );
    }

    let mut stream = UnixStream::connect(&path)
        .await
        .with_context(|| format!("connecting to {}", path.display()))?;

    let json = serde_json::to_vec(cmd).context("serializing IPC command")?;
    stream
        .write_all(&json)
        .await
        .context("writing IPC command")?;
    stream.shutdown().await.context("shutting down IPC stream")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// macOS URL handler installation
// ---------------------------------------------------------------------------

/// Install a minimal .app bundle in ~/Applications that registers the
/// `hutt://` URL scheme on macOS.  The app is a shell script that forwards
/// the URL to the running hutt instance via the IPC socket.
pub fn install_macos_handler() -> Result<()> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let app_dir = PathBuf::from(&home).join("Applications/Hutt Opener.app");
    let contents = app_dir.join("Contents");
    let macos_dir = contents.join("MacOS");

    std::fs::create_dir_all(&macos_dir)
        .with_context(|| format!("creating {}", macos_dir.display()))?;

    // --- Info.plist ---
    let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Hutt Opener</string>
    <key>CFBundleIdentifier</key>
    <string>com.hutt.opener</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>CFBundleExecutable</key>
    <string>hutt-open</string>
    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleURLName</key>
            <string>Hutt URL</string>
            <key>CFBundleURLSchemes</key>
            <array>
                <string>hutt</string>
            </array>
        </dict>
    </array>
</dict>
</plist>
"#;
    let plist_path = contents.join("Info.plist");
    std::fs::write(&plist_path, plist)
        .with_context(|| format!("writing {}", plist_path.display()))?;

    // --- Executable shell script ---
    // The script determines the socket path using the same logic as Rust,
    // constructs a JSON IPC command, and sends it via socat or a simple
    // /dev/unix pipe.
    let script = format!(
        r#"#!/bin/bash
# Hutt URL handler — forwards hutt:// URLs to the running instance.
URL="$1"
if [ -z "$URL" ]; then
    exit 0
fi

SOCK="${{XDG_RUNTIME_DIR:-/tmp/hutt-$(id -u).sock}}/hutt.sock"
# Fallback: if XDG_RUNTIME_DIR was not set, the socket is at /tmp/hutt-<uid>.sock
if [ ! -S "$SOCK" ]; then
    SOCK="/tmp/hutt-$(id -u).sock"
fi
if [ -n "$XDG_RUNTIME_DIR" ] && [ -S "$XDG_RUNTIME_DIR/hutt.sock" ]; then
    SOCK="$XDG_RUNTIME_DIR/hutt.sock"
fi

# Escape the URL for JSON (minimal: backslash and double-quote)
ESCAPED=$(printf '%s' "$URL" | sed 's/\\/\\\\/g; s/"/\\"/g')

JSON=$(cat <<EOF
{{"type":"Open","kind":"Message","id":"$ESCAPED"}}
EOF
)

# Prefer socat if available, otherwise try python3
if command -v socat &>/dev/null; then
    printf '%s' "$JSON" | socat - UNIX-CONNECT:"$SOCK"
elif command -v python3 &>/dev/null; then
    python3 -c "
import socket, sys, json
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect('$SOCK')
# Re-parse URL and build proper command
url = '$URL'
if url.startswith('hutt://message/'):
    mid = url[len('hutt://message/'):]
    cmd = json.dumps({{'type': 'Open', 'kind': 'Message', 'id': mid}})
elif url.startswith('hutt://thread/'):
    mid = url[len('hutt://thread/'):]
    cmd = json.dumps({{'type': 'Open', 'kind': 'Thread', 'id': mid}})
elif url.startswith('hutt://search/'):
    q = url[len('hutt://search/'):]
    cmd = json.dumps({{'type': 'Open', 'kind': 'Search', 'query': q}})
else:
    cmd = json.dumps({{'type': 'Open', 'kind': 'Message', 'id': url}})
s.sendall(cmd.encode())
s.close()
"
fi
"#
    );

    let script_path = macos_dir.join("hutt-open");
    std::fs::write(&script_path, script)
        .with_context(|| format!("writing {}", script_path.display()))?;

    // Make the script executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&script_path, perms)
            .with_context(|| format!("chmod {}", script_path.display()))?;
    }

    // Tell Launch Services to re-register the app
    let _ = std::process::Command::new("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister")
        .args(["-f", app_dir.to_str().unwrap_or("")])
        .output();

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers: minimal percent-encoding / decoding (no extra crate needed)
// ---------------------------------------------------------------------------

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(hex_digit(b >> 4));
                out.push(hex_digit(b & 0x0f));
            }
        }
    }
    out
}

fn url_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'A' + n - 10) as char,
        _ => '0',
    }
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

fn parse_query_string(qs: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for pair in qs.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(url_decode(k), url_decode(v));
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_message_url() {
        assert_eq!(
            format_message_url("abc123@example.com"),
            "hutt://message/abc123@example.com"
        );
    }

    #[test]
    fn test_format_thread_url() {
        assert_eq!(
            format_thread_url("abc123@example.com"),
            "hutt://thread/abc123@example.com"
        );
    }

    #[test]
    fn test_format_search_url() {
        assert_eq!(
            format_search_url("from:alice subject:hello world"),
            "hutt://search/from%3Aalice%20subject%3Ahello%20world"
        );
    }

    #[test]
    fn test_format_compose_url() {
        let url = format_compose_url("bob@example.com", "Hello World");
        assert_eq!(
            url,
            "hutt://compose?to=bob%40example.com&subject=Hello%20World"
        );
    }

    #[test]
    fn test_parse_message_url() {
        assert_eq!(
            parse_hutt_url("hutt://message/abc123@example.com"),
            Some(HuttUrl::Message("abc123@example.com".to_string()))
        );
    }

    #[test]
    fn test_parse_thread_url() {
        assert_eq!(
            parse_hutt_url("hutt://thread/abc123@example.com"),
            Some(HuttUrl::Thread("abc123@example.com".to_string()))
        );
    }

    #[test]
    fn test_parse_search_url() {
        assert_eq!(
            parse_hutt_url("hutt://search/from%3Aalice%20subject%3Ahello%20world"),
            Some(HuttUrl::Search(
                "from:alice subject:hello world".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_compose_url() {
        assert_eq!(
            parse_hutt_url("hutt://compose?to=bob%40example.com&subject=Hello%20World"),
            Some(HuttUrl::Compose {
                to: "bob@example.com".to_string(),
                subject: "Hello World".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_invalid_url() {
        assert_eq!(parse_hutt_url("https://example.com"), None);
        assert_eq!(parse_hutt_url("hutt://message/"), None);
        assert_eq!(parse_hutt_url("hutt://unknown/foo"), None);
    }

    #[test]
    fn test_url_encode_decode_roundtrip() {
        let original = "hello world! @#$%^&*()";
        let encoded = url_encode(original);
        let decoded = url_decode(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_ipc_command_json_roundtrip() {
        let cmds = vec![
            IpcCommand::Open(HuttUrlSerde::Message {
                id: "test@example.com".to_string(),
            }),
            IpcCommand::Navigate {
                folder: "/Inbox".to_string(),
            },
            IpcCommand::Quit,
        ];

        for cmd in &cmds {
            let json = serde_json::to_string(cmd).unwrap();
            let parsed: IpcCommand = serde_json::from_str(&json).unwrap();
            let json2 = serde_json::to_string(&parsed).unwrap();
            assert_eq!(json, json2);
        }
    }
}
