use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use lexpr::parse::{KeywordSyntax, NilSymbol, Options};
use lexpr::Value;
use std::path::PathBuf;

use crate::envelope::{Address, Envelope, Flag, ThreadMeta};

/// lexpr parse options configured for mu server's Emacs Lisp-style s-expressions.
fn mu_parse_options() -> Options {
    Options::new()
        .with_keyword_syntax(KeywordSyntax::ColonPrefix)
        .with_nil_symbol(NilSymbol::Special)
}

/// Parse a mu server sexp string into a Value.
pub fn parse_sexp(s: &str) -> Result<Value> {
    lexpr::from_str_custom(s, mu_parse_options())
        .with_context(|| format!("failed to parse sexp: {}", truncate(s, 200)))
}

/// Read a framed sexp response from raw bytes.
///
/// mu server framing: \xfe<hex-length>\xff<sexp-bytes>
/// Returns (parsed Value, bytes consumed) or None if no complete frame found.
pub fn read_frame(buf: &[u8]) -> Result<Option<(Value, usize)>> {
    // Find the frame start marker
    let start = match buf.iter().position(|&b| b == 0xfe) {
        Some(pos) => pos,
        None => return Ok(None),
    };

    // Find the length/data separator
    let sep = match buf[start + 1..].iter().position(|&b| b == 0xff) {
        Some(pos) => start + 1 + pos,
        None => return Ok(None),
    };

    // Parse hex length
    let hex_str = std::str::from_utf8(&buf[start + 1..sep])
        .context("invalid utf-8 in frame length")?;
    let length =
        usize::from_str_radix(hex_str, 16).context("invalid hex length in frame")?;

    let data_start = sep + 1;
    let data_end = data_start + length;

    // Do we have enough data?
    if buf.len() < data_end {
        return Ok(None);
    }

    let sexp_bytes = &buf[data_start..data_end];
    let sexp_str =
        std::str::from_utf8(sexp_bytes).context("invalid utf-8 in sexp data")?;

    let value = parse_sexp(sexp_str)?;

    Ok(Some((value, data_end)))
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

/// Extract a string value from a plist by keyword key.
pub fn plist_get_str<'a>(plist: &'a Value, key: &str) -> Option<&'a str> {
    let list = plist.as_cons()?;
    let mut iter = list.iter();
    while let Some(item) = iter.next() {
        if let Some(kw) = item.car().as_keyword() {
            if kw == key {
                if let Some(val) = iter.next() {
                    return val.car().as_str();
                }
            }
        }
    }
    None
}

/// Extract a u32 value from a plist by keyword key.
pub fn plist_get_u32(plist: &Value, key: &str) -> Option<u32> {
    let list = plist.as_cons()?;
    let mut iter = list.iter();
    while let Some(item) = iter.next() {
        if let Some(kw) = item.car().as_keyword() {
            if kw == key {
                if let Some(val) = iter.next() {
                    return val.car().as_u64().map(|n| n as u32);
                }
            }
        }
    }
    None
}

/// Extract a boolean (symbol t/nil) from a plist by keyword key.
pub fn plist_get_bool(plist: &Value, key: &str) -> Option<bool> {
    let list = plist.as_cons()?;
    let mut iter = list.iter();
    while let Some(item) = iter.next() {
        if let Some(kw) = item.car().as_keyword() {
            if kw == key {
                if let Some(val) = iter.next() {
                    let car = val.car();
                    if car.is_symbol() {
                        return Some(car.as_symbol() == Some("t"));
                    }
                    return Some(!car.is_nil());
                }
            }
        }
    }
    None
}

/// Extract a sub-plist value from a plist by keyword key.
pub fn plist_get<'a>(plist: &'a Value, key: &str) -> Option<&'a Value> {
    let list = plist.as_cons()?;
    let mut iter = list.iter();
    while let Some(item) = iter.next() {
        if let Some(kw) = item.car().as_keyword() {
            if kw == key {
                if let Some(val) = iter.next() {
                    return Some(val.car());
                }
            }
        }
    }
    None
}

/// Parse an Emacs-style time value (high low micro) into a DateTime<Utc>.
/// Format: (high low micro) where seconds = high * 65536 + low
fn parse_emacs_time(value: &Value) -> Option<DateTime<Utc>> {
    let cons = value.as_cons()?;
    let items: Vec<_> = cons.iter().map(|pair| pair.car().clone()).collect();
    if items.len() >= 2 {
        let high = items[0].as_u64()?;
        let low = items[1].as_u64()?;
        let seconds = (high * 65536 + low) as i64;
        Utc.timestamp_opt(seconds, 0).single()
    } else {
        None
    }
}

/// Parse an address plist like (:email "foo@bar" :name "Foo")
fn parse_address(value: &Value) -> Option<Address> {
    let email = plist_get_str(value, "email")?.to_string();
    let name = plist_get_str(value, "name").map(|s| s.to_string());
    Some(Address { name, email })
}

/// Parse a list of address plists.
fn parse_addresses(value: &Value) -> Vec<Address> {
    match value.as_cons() {
        Some(cons) => cons
            .iter()
            .filter_map(|pair| parse_address(pair.car()))
            .collect(),
        None => vec![],
    }
}

/// Parse flags from a list of symbols like (seen list flagged).
fn parse_flags(value: &Value) -> Vec<Flag> {
    match value.as_cons() {
        Some(cons) => cons
            .iter()
            .filter_map(|pair| {
                pair.car().as_symbol().and_then(Flag::from_symbol)
            })
            .collect(),
        None => vec![],
    }
}

/// Parse thread metadata from the :meta plist.
fn parse_thread_meta(value: &Value) -> ThreadMeta {
    ThreadMeta {
        level: plist_get_u32(value, "level").unwrap_or(0),
        root: plist_get_bool(value, "root").unwrap_or(true),
        thread_subject: plist_get_bool(value, "thread-subject").unwrap_or(true),
    }
}

/// Parse a single envelope from a mu find response header plist.
pub fn parse_envelope(value: &Value) -> Result<Envelope> {
    let docid = plist_get_u32(value, "docid")
        .context("missing docid in envelope")?;
    let message_id = plist_get_str(value, "message-id")
        .unwrap_or("")
        .to_string();
    let subject = plist_get_str(value, "subject")
        .unwrap_or("(no subject)")
        .to_string();
    let maildir = plist_get_str(value, "maildir")
        .unwrap_or("")
        .to_string();
    let path = plist_get_str(value, "path")
        .map(PathBuf::from)
        .unwrap_or_default();

    let date = plist_get(value, "date")
        .and_then(parse_emacs_time)
        .unwrap_or_else(Utc::now);

    let from = plist_get(value, "from")
        .map(parse_addresses)
        .unwrap_or_default();
    let to = plist_get(value, "to")
        .map(parse_addresses)
        .unwrap_or_default();
    let flags = plist_get(value, "flags")
        .map(parse_flags)
        .unwrap_or_default();
    let thread_meta = plist_get(value, "meta")
        .map(parse_thread_meta)
        .unwrap_or_default();

    Ok(Envelope {
        docid,
        message_id,
        subject,
        from,
        to,
        date,
        flags,
        maildir,
        path,
        thread_meta,
    })
}

/// Parse the :headers list from a find response into a Vec<Envelope>.
pub fn parse_find_response(value: &Value) -> Result<Vec<Envelope>> {
    let headers = plist_get(value, "headers");
    match headers {
        Some(list) => {
            if let Some(cons) = list.as_cons() {
                cons.iter()
                    .map(|pair| parse_envelope(pair.car()))
                    .collect()
            } else {
                Ok(vec![])
            }
        }
        None => {
            // Could be a (:found N ...) response with no headers
            Ok(vec![])
        }
    }
}

/// Check if a response is an error.
///
/// mu sends errors as `(:error <code> :message "text")`.  The error code
/// can be a number or a string depending on the mu version, so we check
/// for the `:error` key with any value type and prefer `:message` for the
/// human-readable description.
pub fn is_error(value: &Value) -> Option<String> {
    if plist_get(value, "error").is_some() {
        // Prefer :message field for descriptive text
        if let Some(msg) = plist_get_str(value, "message") {
            return Some(msg.to_string());
        }
        // Fall back to the :error value itself
        if let Some(s) = plist_get_str(value, "error") {
            return Some(s.to_string());
        }
        if let Some(code) = plist_get_u32(value, "error") {
            return Some(format!("error code {}", code));
        }
        return Some("unknown error".to_string());
    }
    None
}

/// Check if this is a :found response (end of find results).
pub fn is_found(value: &Value) -> Option<u32> {
    plist_get_u32(value, "found")
}

/// Check if this is a :pong response.
pub fn is_pong(value: &Value) -> bool {
    plist_get_str(value, "pong").is_some()
}

/// Check if this is an :erase response.
pub fn is_erase(value: &Value) -> bool {
    plist_get_bool(value, "erase").unwrap_or(false)
}

/// Check if this is an :update response (from move/flag operations).
pub fn is_update(value: &Value) -> bool {
    plist_get(value, "update").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_frame() {
        // Simulate: \xfe + "c" (hex for 12) + \xff + "(:pong \"mu\")"
        let sexp = "(:pong \"mu\")";
        let hex_len = format!("{:x}", sexp.len());
        let mut buf = vec![0xfe];
        buf.extend_from_slice(hex_len.as_bytes());
        buf.push(0xff);
        buf.extend_from_slice(sexp.as_bytes());

        let result = read_frame(&buf).unwrap().unwrap();
        assert!(is_pong(&result.0));
    }

    #[test]
    fn test_read_frame_incomplete() {
        let buf = vec![0xfe, b'a', 0xff]; // claims 10 bytes but has 0
        let result = read_frame(&buf).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_envelope_from_real_sexp() {
        let sexp = r#"(:path "/mail/Inbox/cur/123:2,S" :date (27028 6999 0) :flags (seen list) :from ((:email "alice@example.com" :name "Alice")) :to ((:email "bob@example.com")) :subject "Hello World" :message-id "abc@example.com" :maildir "/Inbox" :docid 42 :meta (:level 0 :root t :thread-subject t))"#;

        let value = parse_sexp(sexp).unwrap();
        let env = parse_envelope(&value).unwrap();

        assert_eq!(env.docid, 42);
        assert_eq!(env.subject, "Hello World");
        assert_eq!(env.message_id, "abc@example.com");
        assert_eq!(env.from[0].email, "alice@example.com");
        assert_eq!(env.from[0].name.as_deref(), Some("Alice"));
        assert_eq!(env.to[0].email, "bob@example.com");
        assert_eq!(env.maildir, "/Inbox");
        assert!(env.flags.contains(&Flag::Seen));
        assert!(env.flags.contains(&Flag::List));
        assert_eq!(env.thread_meta.level, 0);
        assert!(env.thread_meta.root);
    }

    #[test]
    fn test_parse_real_mu_headers_response() {
        // Actual sexp from mu server (captured from test run)
        let sexp = r#"(:headers ((:path "/mail/Trash/cur/123:2,S" :size 75490 :changed (27030 44466 0) :date (27028 6999 0) :flags (seen list) :from ((:email "news@topazlabs.com" :name "Topaz Labs")) :list "" :message-id "01KHN8NJDKVYWPGK8D09A8RK73@klaviyomail.com" :priority normal :subject "Get better slow motion footage" :to ((:email "a@almnck.com" :name "Danny O'Brie")) :maildir "/Trash" :docid 14 :meta (:path "2:z" :level 0 :date "n69941b57" :data-tstamp (0 0 0) :root t :thread-subject t))))"#;

        let value = parse_sexp(sexp).unwrap();
        let envelopes = parse_find_response(&value).unwrap();
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].docid, 14);
        assert_eq!(envelopes[0].subject, "Get better slow motion footage");
        assert_eq!(envelopes[0].from[0].name.as_deref(), Some("Topaz Labs"));
    }

    #[test]
    fn test_parse_emacs_time() {
        // (27028 6999 0) -> 27028 * 65536 + 6999 = 1771469927
        let sexp = "(27028 6999 0)";
        let value = parse_sexp(sexp).unwrap();
        let dt = parse_emacs_time(&value).unwrap();
        assert_eq!(dt.timestamp(), 27028 * 65536 + 6999);
    }

    #[test]
    fn test_is_erase() {
        let value = parse_sexp("(:erase t)").unwrap();
        assert!(is_erase(&value));
    }

    #[test]
    fn test_is_found() {
        let value = parse_sexp("(:found 3 :query \"\" :maxnum 3)").unwrap();
        assert_eq!(is_found(&value), Some(3));
    }
}
