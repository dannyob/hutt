use anyhow::{Context, Result};

/// Expand `~/` prefix in a maildir root path.
pub fn expand_maildir_root(maildir: &str) -> String {
    if let Some(rest) = maildir.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{}/{}", home, rest)
    } else {
        maildir.to_string()
    }
}

/// Save a formatted message to the Sent maildir folder.
pub fn save_to_sent(maildir_root: &str, sent_folder: &str, message: &[u8]) -> Result<()> {
    let root = expand_maildir_root(maildir_root);
    let sent_cur = format!("{}{}/cur", root, sent_folder);

    // Ensure the Sent/cur directory exists
    std::fs::create_dir_all(&sent_cur)
        .with_context(|| format!("failed to create {}", sent_cur))?;

    // Maildir filename: time.pid_seq.hostname:2,S (Seen flag)
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let hostname = gethostname();
    let filename = format!(
        "{}.{}_{}.{}:2,S",
        timestamp,
        std::process::id(),
        rand_seq(),
        hostname,
    );
    let path = format!("{}/{}", sent_cur, filename);

    std::fs::write(&path, message).with_context(|| format!("failed to save to {}", path))?;

    Ok(())
}

/// Simple counter for unique maildir filenames within a process.
pub fn rand_seq() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// Get the system hostname (for maildir filenames).
pub fn gethostname() -> String {
    let mut buf = [0u8; 256];
    let ret = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if ret == 0 {
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..len]).to_string()
    } else {
        "localhost".to_string()
    }
}
