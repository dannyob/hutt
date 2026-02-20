use anyhow::{bail, Context, Result};
use lexpr::Value;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

fn debug_log_path() -> Option<&'static str> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| std::env::var("HUTT_LOG").ok())
        .as_deref()
}

macro_rules! mu_log {
    ($($arg:tt)*) => {
        if let Some(path) = debug_log_path() {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                use std::io::Write;
                let _ = writeln!(f, "mu: {}", format_args!($($arg)*));
            }
        }
    };
}

use crate::envelope::Envelope;
use crate::mu_sexp;

pub struct MuClient {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    reader: FrameReader,
}

struct FrameReader {
    stdout: BufReader<ChildStdout>,
    buf: Vec<u8>,
}

impl FrameReader {
    fn new(stdout: BufReader<ChildStdout>) -> Self {
        Self {
            stdout,
            buf: Vec::with_capacity(64 * 1024),
        }
    }

    /// Read the next framed sexp response, skipping comment lines and prompts.
    async fn next_frame(&mut self) -> Result<Value> {
        loop {
            // Try to parse a frame from what we have
            if let Some((value, consumed)) = mu_sexp::read_frame(&self.buf)? {
                self.buf.drain(..consumed);
                return Ok(value);
            }

            // Read more data
            let mut tmp = [0u8; 8192];
            let n = self.stdout.read(&mut tmp).await?;
            if n == 0 {
                bail!("mu server closed stdout");
            }
            self.buf.extend_from_slice(&tmp[..n]);
        }
    }
}

pub struct FindOpts {
    pub threads: bool,
    pub sort_field: String,
    pub descending: bool,
    pub max_num: u32,
    pub include_related: bool,
}

impl Default for FindOpts {
    fn default() -> Self {
        Self {
            threads: true,
            sort_field: "date".to_string(),
            descending: true,
            max_num: 500,
            include_related: false,
        }
    }
}

/// Check if a mu database exists at `muhome`, and if not, run `mu init` and `mu index`.
/// Called before starting the mu server for an account.
pub async fn ensure_mu_database(muhome: Option<&str>, maildir: &str) -> Result<()> {
    let muhome = match muhome {
        Some(path) => path,
        None => return Ok(()), // system default, assume already initialized
    };

    let db_dir = std::path::PathBuf::from(muhome).join("xapian");
    if db_dir.is_dir() {
        return Ok(()); // database exists
    }

    // Expand ~ in maildir
    let expanded_maildir = if let Some(rest) = maildir.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}/{}", home, rest)
    } else {
        maildir.to_string()
    };

    // Ensure muhome directory exists
    std::fs::create_dir_all(muhome)
        .with_context(|| format!("failed to create muhome directory {}", muhome))?;

    // Run mu init (capture output to avoid corrupting the TUI)
    let output = tokio::process::Command::new("mu")
        .args(["init", "--muhome", muhome, "--maildir", &expanded_maildir])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .context("failed to run mu init")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("mu init failed: {}", stderr.trim());
    }

    // Run mu index (capture output)
    let output = tokio::process::Command::new("mu")
        .args(["index", "--muhome", muhome])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .context("failed to run mu index")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("mu index failed: {}", stderr.trim());
    }

    Ok(())
}

impl MuClient {
    /// Spawn a mu server process and wait for the initial pong.
    /// If `muhome` is Some, passes `--muhome <path>` to select a specific mu database.
    pub async fn start(muhome: Option<&str>) -> Result<Self> {
        let mut cmd = Command::new("mu");
        cmd.arg("server");
        if let Some(path) = muhome {
            cmd.args(["--muhome", path]);
        }
        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("failed to spawn mu server")?;

        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;

        let mut client = Self {
            child,
            stdin: BufWriter::new(stdin),
            reader: FrameReader::new(BufReader::new(stdout)),
        };

        // Wait for initial welcome, then ping
        client.ping().await?;
        Ok(client)
    }

    /// Send a raw command string to mu server.
    async fn send(&mut self, cmd: &str) -> Result<()> {
        self.stdin
            .write_all(cmd.as_bytes())
            .await
            .context("write to mu stdin")?;
        self.stdin
            .write_all(b"\n")
            .await
            .context("write newline to mu stdin")?;
        self.stdin.flush().await.context("flush mu stdin")?;
        Ok(())
    }

    /// Read the next meaningful response (skipping :erase markers).
    async fn recv(&mut self) -> Result<Value> {
        loop {
            let value = self.reader.next_frame().await?;
            if mu_sexp::is_erase(&value) {
                continue;
            }
            if let Some(err) = mu_sexp::is_error(&value) {
                bail!("mu server error: {}", err);
            }
            return Ok(value);
        }
    }

    /// Like recv() but with a timeout.  Returns None on timeout.
    #[allow(dead_code)]
    async fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<Value>> {
        loop {
            match tokio::time::timeout(timeout, self.reader.next_frame()).await {
                Ok(Ok(value)) => {
                    if mu_sexp::is_erase(&value) {
                        continue;
                    }
                    if let Some(err) = mu_sexp::is_error(&value) {
                        bail!("mu server error: {}", err);
                    }
                    return Ok(Some(value));
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => return Ok(None), // timeout
            }
        }
    }

    pub async fn ping(&mut self) -> Result<()> {
        self.send("(ping)").await?;
        let resp = self.recv().await?;
        if !mu_sexp::is_pong(&resp) {
            bail!("expected pong, got: {:?}", resp);
        }
        Ok(())
    }

    /// Run a find query and collect all envelope results.
    pub async fn find(&mut self, query: &str, opts: &FindOpts) -> Result<Vec<Envelope>> {
        let mut cmd = format!(
            "(find :query \"{}\" :sortfield :{} :maxnum {}",
            escape_string(query),
            opts.sort_field,
            opts.max_num,
        );
        if opts.threads {
            cmd.push_str(" :threads t");
        }
        if opts.descending {
            cmd.push_str(" :descending t");
        }
        if opts.include_related {
            cmd.push_str(" :include-related t");
        }
        cmd.push(')');

        self.send(&cmd).await?;

        let mut envelopes = Vec::new();
        loop {
            let value = self.reader.next_frame().await?;
            if mu_sexp::is_erase(&value) {
                continue;
            }
            if let Some(err) = mu_sexp::is_error(&value) {
                bail!("mu find error: {}", err);
            }
            if mu_sexp::is_found(&value).is_some() {
                break;
            }
            // This should be a :headers response
            let mut batch = mu_sexp::parse_find_response(&value)?;
            envelopes.append(&mut batch);
        }
        Ok(envelopes)
    }

    /// Run a find query and return envelopes plus the total match count.
    /// Used for live preview during smart folder creation.
    pub async fn find_preview(
        &mut self,
        query: &str,
        max_num: u32,
    ) -> Result<(Vec<Envelope>, u32)> {
        let cmd = format!(
            "(find :query \"{}\" :sortfield :date :maxnum {} :threads t :descending t)",
            escape_string(query),
            max_num,
        );
        self.send(&cmd).await?;

        let mut envelopes = Vec::new();
        loop {
            let value = self.reader.next_frame().await?;
            if mu_sexp::is_erase(&value) {
                continue;
            }
            if let Some(err) = mu_sexp::is_error(&value) {
                bail!("mu find error: {}", err);
            }
            if let Some(count) = mu_sexp::is_found(&value) {
                return Ok((envelopes, count));
            }
            let mut batch = mu_sexp::parse_find_response(&value)?;
            envelopes.append(&mut batch);
        }
    }

    /// Move a message to a different maildir and/or change flags.
    /// Returns the new docid assigned by mu after the move.
    pub async fn move_msg(
        &mut self,
        docid: u32,
        maildir: Option<&str>,
        flags: Option<&str>,
    ) -> Result<u32> {
        let mut cmd = format!("(move :docid {}", docid);
        if let Some(md) = maildir {
            cmd.push_str(&format!(" :maildir \"{}\"", escape_string(md)));
        }
        if let Some(f) = flags {
            cmd.push_str(&format!(" :flags \"{}\"", escape_string(f)));
        }
        cmd.push_str(" :rename t)");

        self.send(&cmd).await?;
        let resp = self.recv().await?;
        // The :update response contains the updated envelope with the new docid
        if let Some(update) = mu_sexp::plist_get(&resp, "update") {
            if let Some(new_docid) = mu_sexp::plist_get_u32(update, "docid") {
                return Ok(new_docid);
            }
        }
        // Fallback: return original docid if we can't parse the response
        Ok(docid)
    }

    /// Send the `(index)` command to mu server without waiting for the
    /// response.  Call `poll_index_frame()` to read responses one at a
    /// time from the event loop.
    pub async fn start_index(&mut self) -> Result<()> {
        mu_log!("index: sent (index)");
        self.send("(index)").await
    }

    /// Read one frame from the mu server during an index operation.
    ///
    /// Returns:
    /// - `Ok(true)`  — indexing is complete
    /// - `Ok(false)` — progress update, call again
    /// - `Err(_)`    — error (including from mu server)
    pub async fn poll_index_frame(&mut self) -> Result<bool> {
        let value = self.reader.next_frame().await?;
        mu_log!("index: recv {:?}", value);

        if mu_sexp::is_erase(&value) {
            return Ok(false);
        }
        if let Some(err) = mu_sexp::is_error(&value) {
            mu_log!("index: error: {}", err);
            bail!("mu index error: {}", err);
        }
        if mu_sexp::plist_get(&value, "index").is_some() {
            mu_log!("index: complete (:index)");
            return Ok(true);
        }
        if mu_sexp::plist_get(&value, "info").is_some() {
            mu_log!("index: complete (:info)");
            return Ok(true);
        }
        if mu_sexp::is_update(&value) {
            return Ok(false); // progress update
        }
        mu_log!("index: unexpected response, skipping");
        Ok(false)
    }

    pub async fn quit(&mut self) -> Result<()> {
        let _ = self.send("(quit)").await;
        let _ = self.child.wait().await;
        Ok(())
    }
}

impl Drop for MuClient {
    fn drop(&mut self) {
        // Best-effort kill
        let _ = self.child.start_kill();
    }
}

/// Escape a string for inclusion in an s-expression.
fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
