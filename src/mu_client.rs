use anyhow::{bail, Context, Result};
use lexpr::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

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
    pub async fn index(&mut self, lazy: bool) -> Result<()> {
        let cmd = if lazy {
            "(index :lazy t)".to_string()
        } else {
            "(index)".to_string()
        };
        self.send(&cmd).await?;
        // Index sends progress updates then a final (:info index ...) message.
        // For now just drain until we see something that's not an index update.
        loop {
            let value = self.recv().await?;
            // Index complete when we get an :index response
            if mu_sexp::plist_get(&value, "index").is_some() {
                break;
            }
            if mu_sexp::plist_get(&value, "info").is_some() {
                break;
            }
            // Could be progress updates, keep reading
        }
        Ok(())
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
