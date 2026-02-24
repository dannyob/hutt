//! URI schemes and IPC for hutt.
//!
//! ## URI Design
//!
//! hutt uses standard URI schemes where they exist:
//!
//! - `mid:<message-id>` — open a message (RFC 2392)
//! - `mid:<message-id>?view=thread` — open a message's thread
//! - `message:<message-id>` — open a message (IANA provisional, Apple Mail)
//! - `mailto:addr?subject=text` — compose (RFC 6068)
//!
//! For app-specific operations with no standard scheme:
//!
//! - `hutt:search?q=<query>[&account=<name>]` — run a search
//! - `hutt:navigate?folder=<path>[&account=<name>]` — switch to a folder
//!
//! The `account` parameter is optional; when omitted, the active account
//! is used. For `mid:` URLs, hutt searches all accounts since Message-IDs
//! are globally unique (RFC 2822).
//!
//! Legacy `hutt://` URLs (with double slash) are still accepted for
//! backwards compatibility.

use anyhow::{bail, Context, Result};
use arboard::Clipboard;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

// ---------------------------------------------------------------------------
// URI scheme types
// ---------------------------------------------------------------------------

/// Parsed representation of a hutt-understood URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HuttUrl {
    /// Open a message by Message-ID.
    Message { id: String, account: Option<String> },
    /// Open a thread by Message-ID.
    Thread { id: String, account: Option<String> },
    /// Run a search query.
    Search { query: String, account: Option<String> },
    /// Open a compose window.
    Compose { to: String, subject: String, account: Option<String> },
}

// ---------------------------------------------------------------------------
// URI formatting (output — clipboard, display)
// ---------------------------------------------------------------------------

/// Format a `mid:<message-id>` URI (RFC 2392).
pub fn format_message_url(message_id: &str) -> String {
    format!("mid:{}", message_id)
}

/// Format a `mid:<message-id>?view=thread` URI.
pub fn format_thread_url(message_id: &str) -> String {
    format!("mid:{}?view=thread", message_id)
}


// ---------------------------------------------------------------------------
// URI parsing (input — IPC, URL handler, clipboard)
// ---------------------------------------------------------------------------

/// Parse a URI into a `HuttUrl`.
///
/// Accepts:
/// - `mid:<message-id>[?view=thread][&account=name]`
/// - `message:<message-id>` or `message://<message-id>`
/// - `mailto:addr[?subject=text&account=name]`
/// - `hutt:search?q=query[&account=name]`
/// - `hutt:navigate?folder=path[&account=name]`
/// - Legacy: `hutt://message/id`, `hutt://thread/id`, `hutt://search/q`, `hutt://compose?...`
pub fn parse_url(url: &str) -> Option<HuttUrl> {
    // mid:<message-id>[?view=thread]
    if let Some(rest) = url.strip_prefix("mid:") {
        let (id, qs) = split_query(rest);
        if id.is_empty() {
            return None;
        }
        let params = parse_query_string(qs);
        let account = params.get("account").cloned();
        if params.get("view").map(|v| v.as_str()) == Some("thread") {
            return Some(HuttUrl::Thread { id: id.to_string(), account });
        }
        return Some(HuttUrl::Message { id: id.to_string(), account });
    }

    // message:<message-id> or message://<message-id> (Apple Mail)
    if let Some(rest) = url.strip_prefix("message:") {
        let rest = rest.strip_prefix("//").unwrap_or(rest);
        // Apple Mail percent-encodes angle brackets: %3C...%3E
        let id = url_decode(rest);
        let id = id.strip_prefix('<').unwrap_or(&id);
        let id = id.strip_suffix('>').unwrap_or(id);
        if id.is_empty() {
            return None;
        }
        return Some(HuttUrl::Message { id: id.to_string(), account: None });
    }

    // mailto:addr[?subject=text]
    if let Some(rest) = url.strip_prefix("mailto:") {
        let (addr, qs) = split_query(rest);
        let params = parse_query_string(qs);
        let to = url_decode(addr);
        let subject = params.get("subject").cloned().unwrap_or_default();
        let account = params.get("account").cloned();
        return Some(HuttUrl::Compose { to, subject, account });
    }

    // hutt:search?q=... and hutt:navigate?folder=...
    if let Some(rest) = url.strip_prefix("hutt:") {
        // Strip optional // for backwards compat
        let rest = rest.strip_prefix("//").unwrap_or(rest);
        return parse_hutt_path(rest);
    }

    None
}

/// Parse the path+query portion of a hutt: URI.
/// Handles both new format (search?q=...) and legacy (message/id, thread/id, etc).
fn parse_hutt_path(rest: &str) -> Option<HuttUrl> {
    let (path, qs) = split_query(rest);
    let params = parse_query_string(qs);
    let account = params.get("account").cloned();

    // New format: hutt:search?q=...
    if path == "search" {
        let query = params.get("q").cloned().unwrap_or_default();
        if query.is_empty() {
            return None;
        }
        return Some(HuttUrl::Search { query, account });
    }

    // New format: hutt:navigate?folder=...
    if path == "navigate" {
        // Navigate is handled as a special IPC command, not a HuttUrl.
        // But we still parse it to get the folder for the IPC layer.
        return None;
    }

    // Legacy: hutt://message/<id>
    if let Some(id) = path.strip_prefix("message/") {
        if id.is_empty() { return None; }
        return Some(HuttUrl::Message { id: id.to_string(), account });
    }

    // Legacy: hutt://thread/<id>
    if let Some(id) = path.strip_prefix("thread/") {
        if id.is_empty() { return None; }
        return Some(HuttUrl::Thread { id: id.to_string(), account });
    }

    // Legacy: hutt://search/<encoded-query>
    if let Some(encoded) = path.strip_prefix("search/") {
        let query = url_decode(encoded);
        if query.is_empty() { return None; }
        return Some(HuttUrl::Search { query, account });
    }

    // Legacy: hutt://compose?to=...&subject=...
    if path == "compose" {
        let to = params.get("to").cloned().unwrap_or_default();
        let subject = params.get("subject").cloned().unwrap_or_default();
        return Some(HuttUrl::Compose { to, subject, account });
    }

    None
}

/// Parse a `hutt:navigate?folder=...&account=...` URI, returning (folder, account).
/// Separate from parse_url because Navigate is an IPC command, not a HuttUrl.
pub fn parse_navigate_url(url: &str) -> Option<(String, Option<String>)> {
    let rest = url.strip_prefix("hutt:")?;
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let (path, qs) = split_query(rest);
    if path != "navigate" {
        return None;
    }
    let params = parse_query_string(qs);
    let folder = params.get("folder").cloned()?;
    if folder.is_empty() {
        return None;
    }
    let account = params.get("account").cloned();
    Some((folder, account))
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
    Navigate {
        folder: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        account: Option<String>,
    },
    Quit,
    MuCommand {
        sexp: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        account: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        muhome: Option<String>,
    },
}

/// Response sent back to IPC clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum IpcResponse {
    Ok,
    Error { message: String },
    MuFrames { frames: Vec<String> },
}

/// Serde-friendly mirror of `HuttUrl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum HuttUrlSerde {
    Message {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        account: Option<String>,
    },
    Thread {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        account: Option<String>,
    },
    Search {
        query: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        account: Option<String>,
    },
    Compose {
        to: String,
        subject: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        account: Option<String>,
    },
}

impl From<HuttUrl> for HuttUrlSerde {
    fn from(u: HuttUrl) -> Self {
        match u {
            HuttUrl::Message { id, account } => HuttUrlSerde::Message { id, account },
            HuttUrl::Thread { id, account } => HuttUrlSerde::Thread { id, account },
            HuttUrl::Search { query, account } => HuttUrlSerde::Search { query, account },
            HuttUrl::Compose { to, subject, account } => HuttUrlSerde::Compose { to, subject, account },
        }
    }
}

impl From<HuttUrlSerde> for HuttUrl {
    fn from(s: HuttUrlSerde) -> Self {
        match s {
            HuttUrlSerde::Message { id, account } => HuttUrl::Message { id, account },
            HuttUrlSerde::Thread { id, account } => HuttUrl::Thread { id, account },
            HuttUrlSerde::Search { query, account } => HuttUrl::Search { query, account },
            HuttUrlSerde::Compose { to, subject, account } => HuttUrl::Compose { to, subject, account },
        }
    }
}

/// Determine the IPC socket path.
pub fn socket_path() -> PathBuf {
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
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("removing stale socket {}", path.display()))?;
        }
        let listener =
            UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
        Ok(Self { listener, path })
    }

    /// Accept a single connection, read a JSON-encoded `IpcCommand`,
    /// and return it along with the stream for sending a response.
    pub async fn accept(&self) -> Result<(IpcCommand, UnixStream)> {
        let (mut stream, _addr) = self
            .listener
            .accept()
            .await
            .context("accepting IPC connection")?;

        // Read until the client shuts down their write side
        let mut buf = Vec::with_capacity(4096);
        loop {
            let mut tmp = [0u8; 4096];
            let n = stream.read(&mut tmp).await.context("reading IPC command")?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }

        let cmd: IpcCommand =
            serde_json::from_slice(&buf).context("deserializing IPC command")?;
        Ok((cmd, stream))
    }
}

impl Drop for IpcListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Write a JSON-encoded IpcResponse to a stream.
pub async fn send_response(stream: &mut UnixStream, resp: &IpcResponse) -> Result<()> {
    let json = serde_json::to_vec(resp).context("serializing IPC response")?;
    stream.write_all(&json).await.context("writing IPC response")?;
    stream.shutdown().await.context("shutting down IPC response stream")?;
    Ok(())
}

/// Client side: connect to the running hutt instance, send a command,
/// and read back the response.
pub async fn send_ipc_command(cmd: &IpcCommand) -> Result<IpcResponse> {
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
    // Shut down write side so the server knows we're done sending
    stream.shutdown().await.context("shutting down write side")?;

    // Read the response
    let mut resp_buf = Vec::with_capacity(4096);
    stream
        .read_to_end(&mut resp_buf)
        .await
        .context("reading IPC response")?;

    let resp: IpcResponse =
        serde_json::from_slice(&resp_buf).context("deserializing IPC response")?;
    Ok(resp)
}


// ---------------------------------------------------------------------------
// Helpers: minimal percent-encoding / decoding
// ---------------------------------------------------------------------------

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

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

/// Split a URI string into path and query components at the first `?`.
fn split_query(s: &str) -> (&str, &str) {
    match s.split_once('?') {
        Some((path, qs)) => (path, qs),
        None => (s, ""),
    }
}

fn parse_query_string(qs: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if qs.is_empty() {
        return map;
    }
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

    // ── mid: URLs ──────────────────────────────────────────────

    #[test]
    fn parse_mid_message() {
        assert_eq!(
            parse_url("mid:abc123@example.com"),
            Some(HuttUrl::Message { id: "abc123@example.com".into(), account: None })
        );
    }

    #[test]
    fn parse_mid_thread() {
        assert_eq!(
            parse_url("mid:abc123@example.com?view=thread"),
            Some(HuttUrl::Thread { id: "abc123@example.com".into(), account: None })
        );
    }

    #[test]
    fn parse_mid_with_account() {
        assert_eq!(
            parse_url("mid:abc123@example.com?account=work"),
            Some(HuttUrl::Message { id: "abc123@example.com".into(), account: Some("work".into()) })
        );
        assert_eq!(
            parse_url("mid:abc123@example.com?view=thread&account=work"),
            Some(HuttUrl::Thread { id: "abc123@example.com".into(), account: Some("work".into()) })
        );
    }

    #[test]
    fn parse_mid_empty() {
        assert_eq!(parse_url("mid:"), None);
    }

    // ── message: URLs (Apple Mail) ─────────────────────────────

    #[test]
    fn parse_message_url() {
        assert_eq!(
            parse_url("message:abc@example.com"),
            Some(HuttUrl::Message { id: "abc@example.com".into(), account: None })
        );
    }

    #[test]
    fn parse_message_url_with_slashes() {
        assert_eq!(
            parse_url("message://abc@example.com"),
            Some(HuttUrl::Message { id: "abc@example.com".into(), account: None })
        );
    }

    #[test]
    fn parse_message_url_with_angle_brackets() {
        // Apple Mail uses %3C...%3E for angle brackets
        assert_eq!(
            parse_url("message://%3Cabc@example.com%3E"),
            Some(HuttUrl::Message { id: "abc@example.com".into(), account: None })
        );
    }

    // ── mailto: URLs ───────────────────────────────────────────

    #[test]
    fn parse_mailto() {
        assert_eq!(
            parse_url("mailto:bob@example.com?subject=Hello%20World"),
            Some(HuttUrl::Compose {
                to: "bob@example.com".into(),
                subject: "Hello World".into(),
                account: None,
            })
        );
    }

    #[test]
    fn parse_mailto_bare() {
        assert_eq!(
            parse_url("mailto:bob@example.com"),
            Some(HuttUrl::Compose {
                to: "bob@example.com".into(),
                subject: String::new(),
                account: None,
            })
        );
    }

    // ── hutt: URLs (new format) ────────────────────────────────

    #[test]
    fn parse_hutt_search() {
        assert_eq!(
            parse_url("hutt:search?q=from%3Aalice"),
            Some(HuttUrl::Search { query: "from:alice".into(), account: None })
        );
    }

    #[test]
    fn parse_hutt_search_with_account() {
        assert_eq!(
            parse_url("hutt:search?q=from%3Aalice&account=work"),
            Some(HuttUrl::Search { query: "from:alice".into(), account: Some("work".into()) })
        );
    }

    #[test]
    fn parse_hutt_navigate() {
        assert_eq!(
            parse_navigate_url("hutt:navigate?folder=%2FInbox"),
            Some(("/Inbox".into(), None))
        );
        assert_eq!(
            parse_navigate_url("hutt:navigate?folder=%2FSent&account=work"),
            Some(("/Sent".into(), Some("work".into())))
        );
    }

    // ── Legacy hutt:// URLs ────────────────────────────────────

    #[test]
    fn parse_legacy_message() {
        assert_eq!(
            parse_url("hutt://message/abc@example.com"),
            Some(HuttUrl::Message { id: "abc@example.com".into(), account: None })
        );
    }

    #[test]
    fn parse_legacy_thread() {
        assert_eq!(
            parse_url("hutt://thread/abc@example.com"),
            Some(HuttUrl::Thread { id: "abc@example.com".into(), account: None })
        );
    }

    #[test]
    fn parse_legacy_search() {
        assert_eq!(
            parse_url("hutt://search/from%3Aalice"),
            Some(HuttUrl::Search { query: "from:alice".into(), account: None })
        );
    }

    #[test]
    fn parse_legacy_compose() {
        assert_eq!(
            parse_url("hutt://compose?to=bob%40example.com&subject=Hello"),
            Some(HuttUrl::Compose {
                to: "bob@example.com".into(),
                subject: "Hello".into(),
                account: None,
            })
        );
    }

    // ── Invalid URLs ───────────────────────────────────────────

    #[test]
    fn parse_invalid() {
        assert_eq!(parse_url("https://example.com"), None);
        assert_eq!(parse_url("hutt://message/"), None);
        assert_eq!(parse_url("hutt://unknown/foo"), None);
        assert_eq!(parse_url("hutt:search?q="), None);
    }

    // ── Formatting ─────────────────────────────────────────────

    #[test]
    fn format_mid_message() {
        assert_eq!(format_message_url("abc@example.com"), "mid:abc@example.com");
    }

    #[test]
    fn format_mid_thread() {
        assert_eq!(format_thread_url("abc@example.com"), "mid:abc@example.com?view=thread");
    }

    // ── Roundtrip ──────────────────────────────────────────────

    #[test]
    fn url_decode_basic() {
        assert_eq!(url_decode("hello%20world"), "hello world");
        assert_eq!(url_decode("from%3Aalice"), "from:alice");
        assert_eq!(url_decode("plain"), "plain");
    }

    #[test]
    fn ipc_response_json_roundtrip() {
        let resps = vec![
            IpcResponse::Ok,
            IpcResponse::Error { message: "not found".to_string() },
            IpcResponse::MuFrames {
                frames: vec![
                    "(:docid 42 :subject \"Hello\")".to_string(),
                    "(:docid 43 :subject \"World\")".to_string(),
                ],
            },
        ];
        for resp in &resps {
            let json = serde_json::to_string(resp).unwrap();
            let parsed: IpcResponse = serde_json::from_str(&json).unwrap();
            let json2 = serde_json::to_string(&parsed).unwrap();
            assert_eq!(json, json2);
        }
    }

    #[test]
    fn test_mu_command_serde() {
        let cmd = IpcCommand::MuCommand {
            sexp: "(ping)".to_string(),
            account: Some("work".to_string()),
            muhome: None,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: IpcCommand = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn ipc_command_json_roundtrip() {
        let cmds = vec![
            IpcCommand::Open(HuttUrlSerde::Message {
                id: "test@example.com".to_string(),
                account: None,
            }),
            IpcCommand::Navigate {
                folder: "/Inbox".to_string(),
                account: None,
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
