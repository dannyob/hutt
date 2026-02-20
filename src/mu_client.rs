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

impl MuClient {
    /// Spawn a mu server process and wait for the initial pong.
    pub async fn start() -> Result<Self> {
        let mut child = Command::new("mu")
            .arg("server")
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

    /// Move a message to a different maildir and/or change flags.
    pub async fn move_msg(
        &mut self,
        docid: u32,
        maildir: Option<&str>,
        flags: Option<&str>,
    ) -> Result<()> {
        let mut cmd = format!("(move :docid {}", docid);
        if let Some(md) = maildir {
            cmd.push_str(&format!(" :maildir \"{}\"", escape_string(md)));
        }
        if let Some(f) = flags {
            cmd.push_str(&format!(" :flags \"{}\"", escape_string(f)));
        }
        cmd.push_str(" :rename t)");

        self.send(&cmd).await?;
        // Read and discard update response
        let _resp = self.recv().await?;
        Ok(())
    }

    /// Run mu index.
    ///
    /// The mu server responds with progress updates and then a final
    /// completion message.  We drain responses with a per-message timeout
    /// so a protocol mismatch can't hang the caller forever.
    ///
    /// Note: the `_lazy` parameter is accepted for API compatibility but
    /// ignored — mu server's `(index)` command does not support `:lazy`
    /// in all versions, and a plain `(index)` is fast enough.
    pub async fn index(&mut self, _lazy: bool) -> Result<()> {
        let cmd = "(index)";
        self.send(cmd).await?;

        let timeout = Duration::from_secs(30);
        mu_log!("index: sent {}", cmd);
        loop {
            match self.recv_timeout(timeout).await? {
                Some(value) => {
                    mu_log!("index: recv {:?}", value);
                    // Index complete when we get an :index or :info response
                    if mu_sexp::plist_get(&value, "index").is_some() {
                        mu_log!("index: complete (:index)");
                        return Ok(());
                    }
                    if mu_sexp::plist_get(&value, "info").is_some() {
                        mu_log!("index: complete (:info)");
                        return Ok(());
                    }
                    if mu_sexp::is_update(&value) {
                        continue; // progress update, keep reading
                    }
                    // Unknown response type — log and keep trying
                    mu_log!("index: unexpected response type, skipping");
                }
                None => {
                    mu_log!("index: timed out after {}s", timeout.as_secs());
                    bail!("mu index: timed out after {}s waiting for response", timeout.as_secs());
                }
            }
        }
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
