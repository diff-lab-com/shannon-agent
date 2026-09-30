//! Email (IMAP) data source fetcher.
//!
//! Reads inbox messages over IMAPS (TLS, port 993 by default). Config comes
//! from the installed data source TOML (`~/.shannon/data-sources/email-imap.toml`),
//! matching the catalog install form field names:
//!
//! - `imap_host` (required), `imap_port` (required, e.g. 993),
//! - `username` (required), `password` (required),
//! - `drafts_mailbox` (optional, defaults to `Drafts`) — used by [`append_draft`].
//!
//! Query semantics (the `query` string):
//! - Empty — the most recent `limit` unread messages (SEARCH `SINCE <date> UNSEEN`).
//! - Non-empty — `SUBJECT` contains, server-side SEARCH plus a client-side
//!   case-insensitive re-check on the decoded subject/body.
//!
//! # Pure logic vs. IO
//!
//! Everything testable offline lives in pure functions:
//! [`build_imap_search_query`], [`select_and_decode`] (mailparse MIME
//! decoding), and [`build_draft_rfc822`]. Only the private `fetch_sync` and
//! `append_draft` helpers touch the network, so unit tests never open a
//! socket.
//!
//! # TLS and timeouts
//!
//! We build the connection by hand with `imap::Client::new` instead of any
//! connect helper: `TcpStream::connect_timeout` +
//! `set_read_timeout`/`set_write_timeout` are applied BEFORE the rustls
//! handshake wraps the stream, and the socket timeouts carry through to all
//! later reads/writes (rustls reads through to the TcpStream). `imap` is
//! compiled with `default-features = false` — its only first-party TLS
//! integration is native-tls/openssl, which the workspace forbids (musl
//! builds) — and TLS is supplied out-of-band by rustls-connector 0.23 on the
//! same rustls 0.23/ring line the workspace already locks via reqwest.

use super::{DataSourceError, DataSourceFetcher, DataSourceItem, DataSourceResult};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use imap::types::Flag;
use mailparse::{DispositionType, ParsedMail, parse_header, parse_mail};
use rustls_connector::RustlsConnector;
use std::collections::BTreeMap;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Results returned per query. The `DataSourceFetcher` trait carries no
/// limit parameter, so this is the fixed page size.
pub const DEFAULT_QUERY_LIMIT: usize = 20;

/// How far back an empty (unread) or subject query reaches, in days.
pub const DEFAULT_SEARCH_DAYS: u32 = 14;

/// Decoded body is truncated to this many chars for the result excerpt.
pub const MAX_BODY_CHARS: usize = 4000;

/// TCP connect timeout for the IMAP connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Socket read/write timeout once connected (covers handshake + commands).
const IO_TIMEOUT: Duration = Duration::from_secs(30);

/// Default mailbox [`append_draft`] writes to.
pub const DEFAULT_DRAFTS_MAILBOX: &str = "Drafts";

/// IMAP credentials parsed from the installed config map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub drafts_mailbox: String,
}

/// A message reduced to what the result card shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedMessage {
    /// RFC 2047-decoded subject line.
    pub subject: String,
    /// Decoded text body (plain preferred, HTML stripped), truncated.
    pub body: String,
}

/// IMAP fetcher.
#[derive(Debug, Clone, Copy)]
pub struct ImapFetcher;

/// Authenticated IMAP session over an rustls TLS stream.
type TlsSession = imap::Session<rustls_connector::TlsStream<TcpStream>>;

#[async_trait]
impl DataSourceFetcher for ImapFetcher {
    async fn fetch(
        &self,
        config: &BTreeMap<String, String>,
        query: &str,
    ) -> Result<DataSourceResult, DataSourceError> {
        let cfg = parse_config(config)?;
        let query = query.to_string();
        // The imap crate is synchronous — keep the blocking IO off the async
        // runtime the Tauri commands run on.
        tokio::task::spawn_blocking(move || fetch_sync(&cfg, &query, DEFAULT_QUERY_LIMIT))
            .await
            .map_err(|e| DataSourceError::UpstreamError(format!("IMAP query task failed: {e}")))?
    }
}

/// Parse the installed config map (catalog field names) into [`ImapConfig`].
pub(crate) fn parse_config(
    config: &BTreeMap<String, String>,
) -> Result<ImapConfig, DataSourceError> {
    let get = |key: &str| {
        config
            .get(key)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .ok_or(DataSourceError::MissingConfig(key.to_string()))
    };
    let host = get("imap_host")?.to_string();
    let port_raw = get("imap_port")?;
    let port = port_raw.parse::<u16>().map_err(|_| {
        DataSourceError::UpstreamError(format!("Invalid imap_port value: {port_raw:?}"))
    })?;
    Ok(ImapConfig {
        host,
        port,
        username: get("username")?.to_string(),
        password: get("password")?.to_string(),
        drafts_mailbox: config
            .get("drafts_mailbox")
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .unwrap_or(DEFAULT_DRAFTS_MAILBOX)
            .to_string(),
    })
}

// ---------------------------------------------------------------------------
// Pure query building (offline testable)
// ---------------------------------------------------------------------------

/// Format a date the way IMAP SEARCH `SINCE` expects: `dd-Mon-yyyy`.
fn imap_date_string(date: DateTime<Utc>) -> String {
    date.format("%d-%b-%Y").to_string()
}

/// Build the SEARCH query for "recent unread mail": `SINCE <date> UNSEEN`.
pub fn build_imap_search_query(days: u32) -> String {
    let since = Utc::now() - ChronoDuration::days(i64::from(days));
    format!("SINCE {} UNSEEN", imap_date_string(since))
}

/// Build the SEARCH query for a subject contains-match, reaching `days` back.
pub fn build_subject_search_query(days: u32, subject: &str) -> String {
    let since = Utc::now() - ChronoDuration::days(i64::from(days));
    format!(
        "SINCE {} SUBJECT {}",
        imap_date_string(since),
        quote_imap_string(subject)
    )
}

/// Quote a string as an IMAP quoted string (RFC 3501 §4.3): backslash and
/// double quote are escaped.
fn quote_imap_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        if ch == '\\' || ch == '"' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// Pure MIME decoding (offline testable)
// ---------------------------------------------------------------------------

/// Decode the ENVELOPE subject (raw header value bytes, possibly RFC 2047
/// encoded) into a human-readable string.
fn decode_mime_header(raw: &[u8]) -> String {
    if raw.is_empty() {
        return String::new();
    }
    // Hand the raw value to mailparse as a synthetic Subject header so its
    // RFC 2047 / charset decoding applies (get_value decodes encoded words).
    let mut line = Vec::with_capacity(raw.len() + 9);
    line.extend_from_slice(b"Subject: ");
    line.extend_from_slice(raw);
    match parse_header(&line) {
        Ok((header, _)) => header.get_value(),
        Err(_) => String::from_utf8_lossy(raw).into_owned(),
    }
}

/// Decode the fetched message bytes into a text body: prefer `text/plain`,
/// fall back to `text/html` with tags stripped. Transfer encodings (base64,
/// quoted-printable) and charsets are handled by mailparse. Trailing
/// whitespace (the final CRLF, boundary padding) is trimmed.
fn decode_body(bytes: &[u8]) -> String {
    let lossy = || String::from_utf8_lossy(bytes).into_owned();
    let mail = match parse_mail(bytes) {
        Ok(mail) => mail,
        Err(_) => return lossy(),
    };
    let text = match find_text_part(&mail) {
        Some(part) => {
            let is_html = part.ctype.mimetype.eq_ignore_ascii_case("text/html");
            match part.get_body() {
                Ok(text) if is_html => strip_html(&text),
                Ok(text) => text,
                Err(_) => lossy(),
            }
        }
        // No recognized text part — fall back to the root body or raw bytes.
        None => mail.get_body().unwrap_or_else(|_| lossy()),
    };
    text.trim_end().to_string()
}

/// Find the best readable text part of a (possibly multipart) message:
/// `text/plain` wins, `text/html` is the fallback. Attachment parts are
/// skipped.
fn find_text_part<'a>(mail: &'a ParsedMail<'a>) -> Option<&'a ParsedMail<'a>> {
    let is_attachment = mail.get_content_disposition().disposition == DispositionType::Attachment;
    let mimetype = mail.ctype.mimetype.to_lowercase();
    if !is_attachment && (mimetype == "text/plain" || mimetype == "text/html") {
        return Some(mail);
    }
    // Multipart — prefer a direct plain child, then a direct html child,
    // then recurse (e.g. multipart/alternative inside multipart/mixed).
    for part in &mail.subparts {
        if part.ctype.mimetype.eq_ignore_ascii_case("text/plain")
            && part.get_content_disposition().disposition != DispositionType::Attachment
        {
            return Some(part);
        }
    }
    for part in &mail.subparts {
        if part.ctype.mimetype.eq_ignore_ascii_case("text/html")
            && part.get_content_disposition().disposition != DispositionType::Attachment
        {
            return Some(part);
        }
    }
    for part in &mail.subparts {
        if let Some(deep) = find_text_part(part) {
            return Some(deep);
        }
    }
    None
}

/// Decode the subject + body pair for one message.
///
/// `subject` is the raw ENVELOPE subject value; `body_bytes` is the full
/// RFC 822 message fetched with `BODY.PEEK[]`. The body excerpt is truncated
/// to [`MAX_BODY_CHARS`] chars (char-boundary safe).
pub fn select_and_decode(subject: &[u8], body_bytes: &[u8]) -> DecodedMessage {
    let subject = sanitize_header_value(&decode_mime_header(subject));
    let body = truncate_chars(&decode_body(body_bytes), MAX_BODY_CHARS);
    DecodedMessage { subject, body }
}

/// Remove `<script>`/`<style>` blocks (including their content), strip the
/// remaining tags, decode the common HTML entities, and collapse whitespace.
fn strip_html(html: &str) -> String {
    let without_blocks = remove_script_style_blocks(html);
    let without_tags = strip_tags(&without_blocks);
    let decoded = decode_entities(&without_tags);
    collapse_whitespace(&decoded)
}

/// Cut `<script>…</script>` and `<style>…</style>` (unterminated block drops
/// the rest of the input). Scanning is ASCII-case-insensitive, so byte
/// offsets stay valid.
fn remove_script_style_blocks(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    for tag in ["script", "style"] {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        let mut from = 0usize;
        while let Some(rel) = lower[from..].find(&open) {
            let start = from + rel;
            let content_start = start + open.len();
            match lower[content_start..].find(&close) {
                Some(p) => {
                    let end = content_start + p + close.len();
                    cuts.push((start, end));
                    from = end;
                }
                None => {
                    cuts.push((start, input.len()));
                    break;
                }
            }
        }
    }
    if cuts.is_empty() {
        return input.to_string();
    }
    cuts.sort_unstable();
    let mut out = String::with_capacity(input.len());
    let mut pos = 0usize;
    for (start, end) in cuts {
        if start >= pos {
            out.push_str(&input[pos..start]);
            pos = pos.max(end);
        }
    }
    out.push_str(&input[pos..]);
    out
}

/// Drop everything between `<` and `>` (depth-tracked; stray `>` passes through).
fn strip_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut depth = 0usize;
    for ch in input.chars() {
        match ch {
            '<' => {
                depth += 1;
                out.push(' ');
            }
            '>' => depth = depth.saturating_sub(1),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Decode the handful of entities mail bodies actually rely on. `&amp;` is
/// decoded last so `&amp;lt;` yields `&lt;`, not `<`.
fn decode_entities(input: &str) -> String {
    input
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Truncate to at most `max` chars on char boundaries.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{truncated}…")
}

/// Strip CR/LF (and collapse any whitespace runs) from a header value —
/// newlines have no business in a single-line value and would allow header
/// injection in the draft builder.
fn sanitize_header_value(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Pure draft building (offline testable)
// ---------------------------------------------------------------------------

/// Encode a header value for the wire: ASCII passes through (sanitized),
/// anything else becomes a single RFC 2047 B-encoded word. (Long non-ASCII
/// subjects exceed the 75-char encoded-word limit in one word — mail clients
/// tolerate this; splitting is left as follow-up work.)
fn encode_mime_header_value(value: &str) -> String {
    let sanitized = sanitize_header_value(value);
    if sanitized.is_ascii() {
        return sanitized;
    }
    format!("=?utf-8?B?{}?=", BASE64.encode(sanitized.as_bytes()))
}

/// Build a complete RFC 822 draft message with UTF-8 base64 body and reply
/// threading headers. `in_reply_to` (a message-id) populates both
/// `In-Reply-To` and `References` so mail clients thread the reply.
///
/// Line endings are CRLF per RFC 822/5322.
pub fn build_draft_rfc822(
    to: &str,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
) -> String {
    let mut msg = String::with_capacity(body.len() + 256);
    msg.push_str(&format!("To: {}\r\n", sanitize_header_value(to)));
    msg.push_str(&format!(
        "Subject: {}\r\n",
        encode_mime_header_value(subject)
    ));
    msg.push_str(&format!("Date: {}\r\n", Utc::now().to_rfc2822()));
    msg.push_str(&format!(
        "Message-ID: <{}@shannon.local>\r\n",
        uuid::Uuid::new_v4()
    ));
    if let Some(msg_id) = in_reply_to.map(str::trim).filter(|v| !v.is_empty()) {
        msg.push_str(&format!("In-Reply-To: {msg_id}\r\n"));
        msg.push_str(&format!("References: {msg_id}\r\n"));
    }
    msg.push_str("MIME-Version: 1.0\r\n");
    msg.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    msg.push_str("Content-Transfer-Encoding: base64\r\n");
    msg.push_str("\r\n");
    // base64 body wrapped at 76 chars (RFC 2045 line limit).
    let encoded = BASE64.encode(body.as_bytes());
    for chunk in encoded.as_bytes().chunks(76) {
        msg.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        msg.push_str("\r\n");
    }
    msg
}

// ---------------------------------------------------------------------------
// IO — the only functions here that open a socket
// ---------------------------------------------------------------------------

/// Connect over TLS, authenticate and select INBOX.
///
/// Connection and authentication failures get distinct, actionable messages
/// and never panic.
fn open_session(cfg: &ImapConfig) -> Result<TlsSession, DataSourceError> {
    let addr = (cfg.host.as_str(), cfg.port)
        .to_socket_addrs()
        .map_err(|e| {
            DataSourceError::UpstreamError(format!("Failed to resolve IMAP host {}: {e}", cfg.host))
        })?
        .next()
        .ok_or_else(|| {
            DataSourceError::UpstreamError(format!(
                "IMAP host {} resolved to no addresses",
                cfg.host
            ))
        })?;

    let tcp = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| {
        DataSourceError::UpstreamError(format!(
            "Failed to connect to {}:{}: {e}",
            cfg.host, cfg.port
        ))
    })?;
    tcp.set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| DataSourceError::UpstreamError(format!("Failed to set read timeout: {e}")))?;
    tcp.set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|e| DataSourceError::UpstreamError(format!("Failed to set write timeout: {e}")))?;

    // System certificate store; rustls handshake — no openssl anywhere.
    let connector = RustlsConnector::new_with_native_certs().map_err(|e| {
        DataSourceError::UpstreamError(format!("Failed to load system TLS certificates: {e}"))
    })?;
    let tls = connector.connect(&cfg.host, tcp).map_err(|e| {
        DataSourceError::UpstreamError(format!(
            "TLS handshake with {}:{} failed: {e}",
            cfg.host, cfg.port
        ))
    })?;

    let client = imap::Client::new(tls);
    let mut session = client
        .login(&cfg.username, &cfg.password)
        .map_err(|(e, _)| match e {
            // Tagged NO/BAD to LOGIN means the server rejected the credentials.
            imap::Error::No(_) | imap::Error::Bad(_) => DataSourceError::AuthError,
            other => DataSourceError::UpstreamError(format!(
                "IMAP authentication failed for {} on {}:{}: {other}",
                cfg.username, cfg.host, cfg.port
            )),
        })?;
    session.select("INBOX").map_err(|e| {
        DataSourceError::UpstreamError(format!("Failed to select INBOX on {}: {e}", cfg.host))
    })?;
    Ok(session)
}

/// Blocking query implementation. Pure sibling functions carry all parsing;
/// tests drive them directly (see the `#[cfg(test)]` module).
pub(crate) fn fetch_sync(
    cfg: &ImapConfig,
    query: &str,
    limit: usize,
) -> Result<DataSourceResult, DataSourceError> {
    let mut session = open_session(cfg)?;

    let trimmed = query.trim();
    let search = if trimmed.is_empty() {
        build_imap_search_query(DEFAULT_SEARCH_DAYS)
    } else {
        build_subject_search_query(DEFAULT_SEARCH_DAYS, trimmed)
    };
    let mut uids: Vec<u32> = session
        .uid_search(&search)
        .map_err(|e| DataSourceError::UpstreamError(format!("IMAP SEARCH failed: {e}")))?
        .into_iter()
        .collect();

    // Newest first (highest UID), limited to the page size.
    uids.sort_unstable_by(|a, b| b.cmp(a));
    uids.truncate(limit);

    if uids.is_empty() {
        let _ = session.logout();
        return Ok(DataSourceResult {
            items: Vec::new(),
            total: 0,
            has_more: false,
        });
    }

    let uid_set = uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let fetches = session
        .uid_fetch(&uid_set, "(UID ENVELOPE INTERNALDATE BODY.PEEK[])")
        .map_err(|e| DataSourceError::UpstreamError(format!("IMAP FETCH failed: {e}")))?;

    // Servers must honor SUBJECT, but re-checking keeps semantics exact if a
    // server searches more loosely (e.g. whole-message).
    let needle = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_lowercase())
    };

    let mut items = Vec::new();
    for fetch in fetches.iter() {
        let Some(uid) = fetch.uid else {
            continue;
        };
        // imap-proto 0.10 Envelope: subject/message_id are Option<&[u8]>.
        let subject_raw: &[u8] = fetch.envelope().and_then(|env| env.subject).unwrap_or(b"");
        let message_id: Option<String> = fetch
            .envelope()
            .and_then(|env| env.message_id)
            .map(|raw| sanitize_header_value(&String::from_utf8_lossy(raw)))
            .filter(|m| !m.is_empty());

        let DecodedMessage { subject, body } =
            select_and_decode(subject_raw, fetch.body().unwrap_or(b""));

        if let Some(needle) = &needle {
            let matches =
                subject.to_lowercase().contains(needle) || body.to_lowercase().contains(needle);
            if !matches {
                continue;
            }
        }

        items.push(DataSourceItem {
            id: message_id.unwrap_or_else(|| format!("uid-{uid}")),
            title: if subject.is_empty() {
                "(no subject)".to_string()
            } else {
                subject
            },
            body: Some(body),
            url: Some(format!("imap://{}/{uid}", cfg.host)),
            kind: "message".into(),
            updated_at: fetch.internal_date().map(|d| d.to_rfc3339()),
        });
    }

    let _ = session.logout();
    Ok(DataSourceResult {
        total: items.len(),
        has_more: false,
        items,
    })
}

/// Append a fully-formed RFC 822 message (see [`build_draft_rfc822`]) to the
/// drafts mailbox with the `\Draft` flag set. B6' will call this to save
/// reply drafts; the real APPEND cannot be unit-tested offline, so only
/// [`build_draft_rfc822`] is covered by tests.
pub fn append_draft(cfg: &ImapConfig, rfc822: &str) -> Result<String, DataSourceError> {
    let mut session = open_session(cfg)?;
    let result = session.append_with_flags(&cfg.drafts_mailbox, rfc822.as_bytes(), &[Flag::Draft]);
    let _ = session.logout();
    result.map_err(|e| {
        DataSourceError::UpstreamError(format!(
            "Failed to append draft to '{}' on {}: {e}",
            cfg.drafts_mailbox, cfg.host
        ))
    })?;
    Ok(format!("Draft appended to {}", cfg.drafts_mailbox))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // -- search query building -------------------------------------------------

    #[test]
    fn imap_date_string_uses_dd_mon_yyyy() {
        let date = Utc.with_ymd_and_hms(2026, 9, 30, 12, 0, 0).unwrap();
        assert_eq!(imap_date_string(date), "30-Sep-2026");
        let date = Utc.with_ymd_and_hms(2026, 1, 5, 0, 0, 0).unwrap();
        assert_eq!(imap_date_string(date), "05-Jan-2026");
    }

    #[test]
    fn build_imap_search_query_is_since_unseen() {
        let query = build_imap_search_query(14);
        assert!(query.starts_with("SINCE "), "got: {query}");
        assert!(query.ends_with(" UNSEEN"), "got: {query}");
        let expected_date = imap_date_string(Utc::now() - ChronoDuration::days(14));
        assert_eq!(query, format!("SINCE {expected_date} UNSEEN"));
    }

    #[test]
    fn build_subject_search_query_escapes_imap_string() {
        let query = build_subject_search_query(30, r#"say "hi" now"#);
        let expected_date = imap_date_string(Utc::now() - ChronoDuration::days(30));
        assert_eq!(
            query,
            format!("SINCE {expected_date} SUBJECT \"say \\\"hi\\\" now\"")
        );
        assert_eq!(quote_imap_string("plain"), "\"plain\"");
        assert_eq!(quote_imap_string("back\\slash"), "\"back\\\\slash\"");
    }

    // -- select_and_decode ------------------------------------------------------

    #[test]
    fn decodes_quoted_printable_body_and_encoded_subject() {
        let subject: &[u8] = b"=?utf-8?B?w4NtbA==?="; // "Ãml"
        let raw = concat!(
            "Subject: ignored\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "Content-Transfer-Encoding: quoted-printable\r\n",
            "\r\n",
            "Caf=C3=A9 r=C3=A9sum=C3=A9 for you\r\n"
        );
        let decoded = select_and_decode(subject, raw.as_bytes());
        assert_eq!(decoded.subject, "Ãml");
        assert_eq!(decoded.body, "Café résumé for you");
    }

    #[test]
    fn multipart_prefers_text_plain_over_html() {
        let raw = concat!(
            "Subject: Report\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/alternative; boundary=BOUND\r\n",
            "\r\n",
            "--BOUND\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "Plain says hello\r\n",
            "--BOUND\r\n",
            "Content-Type: text/html; charset=utf-8\r\n",
            "\r\n",
            "<p>HTML says <b>hello</b></p>\r\n",
            "--BOUND--\r\n"
        );
        let decoded = select_and_decode(b"Report", raw.as_bytes());
        assert_eq!(decoded.body, "Plain says hello");
    }

    #[test]
    fn html_only_body_has_tags_and_entities_stripped() {
        let raw = concat!(
            "Subject: n/a\r\n",
            "Content-Type: text/html; charset=utf-8\r\n",
            "\r\n",
            "<html><head><style>p { color: red }</style></head>",
            "<body><p>Deal &amp; done</p><script>evil()</script>",
            "<p>&quot;Quoted&quot; &lt;tag&gt; &#39;tick&#39;</p></body></html>\r\n"
        );
        let decoded = select_and_decode(b"n/a", raw.as_bytes());
        assert_eq!(decoded.body, "Deal & done \"Quoted\" <tag> 'tick'");
    }

    #[test]
    fn base64_body_is_decoded() {
        let body = BASE64.encode("From base64 with care".as_bytes());
        let raw = format!(
            "Subject: enc\r\nContent-Transfer-Encoding: base64\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n"
        );
        let decoded = select_and_decode(b"enc", raw.as_bytes());
        assert_eq!(decoded.body, "From base64 with care");
    }

    #[test]
    fn attachment_parts_are_skipped() {
        let raw = concat!(
            "Subject: with attachment\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=MIX\r\n",
            "\r\n",
            "--MIX\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "The real message body\r\n",
            "--MIX\r\n",
            "Content-Type: application/pdf\r\n",
            "Content-Disposition: attachment; filename=f.pdf\r\n",
            "\r\n",
            "%PDF-fake\r\n",
            "--MIX--\r\n"
        );
        let decoded = select_and_decode(b"with attachment", raw.as_bytes());
        assert!(decoded.body.starts_with("The real message body"));
        assert!(!decoded.body.contains("%PDF"));
    }

    #[test]
    fn body_is_truncated_to_max_chars() {
        let long = "x".repeat(MAX_BODY_CHARS + 500);
        let raw = format!("Subject: long\r\n\r\n{long}\r\n");
        let decoded = select_and_decode(b"long", raw.as_bytes());
        assert_eq!(decoded.body.chars().count(), MAX_BODY_CHARS + 1); // + ellipsis
        assert!(decoded.body.ends_with('…'));
    }

    #[test]
    fn undecodable_subject_falls_back_to_lossy_text() {
        let decoded = select_and_decode(b"plain ascii subject", b"");
        assert_eq!(decoded.subject, "plain ascii subject");
        let decoded = select_and_decode(b"", b"");
        assert_eq!(decoded.subject, "");
    }

    // -- draft building -----------------------------------------------------------

    #[test]
    fn draft_has_standard_headers_and_base64_body() {
        let draft =
            build_draft_rfc822("alice@example.com", "Follow-up", "Line one\nLine two", None);
        assert!(draft.starts_with("To: alice@example.com\r\n"));
        assert!(draft.contains("Subject: Follow-up\r\n"));
        assert!(draft.contains("Date: "));
        assert!(draft.contains("Message-ID: <"));
        assert!(draft.contains("@shannon.local>\r\n"));
        assert!(
            !draft.contains("In-Reply-To:"),
            "no threading header when absent"
        );
        assert!(!draft.contains("References:"));
        assert!(draft.contains("MIME-Version: 1.0\r\n"));
        assert!(draft.contains("Content-Type: text/plain; charset=utf-8\r\n"));
        assert!(draft.contains("Content-Transfer-Encoding: base64\r\n"));
        // Every line ends with CRLF.
        for line in draft.split("\r\n") {
            assert!(
                !line.contains('\n') && !line.contains('\r'),
                "bare CR/LF: {line:?}"
            );
        }
        // Body round-trips through base64.
        let body_b64 = draft
            .split("\r\n\r\n")
            .nth(1)
            .expect("blank-line separated body")
            .replace("\r\n", "");
        let decoded = BASE64.decode(body_b64).expect("valid base64");
        assert_eq!(String::from_utf8(decoded).unwrap(), "Line one\nLine two");
    }

    #[test]
    fn draft_threads_replies_via_in_reply_to() {
        let draft = build_draft_rfc822(
            "bob@example.com",
            "Re: Launch",
            "ok",
            Some("<abc@example.com>"),
        );
        assert!(draft.contains("In-Reply-To: <abc@example.com>\r\n"));
        assert!(draft.contains("References: <abc@example.com>\r\n"));
    }

    #[test]
    fn draft_encodes_non_ascii_subject_and_strips_header_injection() {
        let draft = build_draft_rfc822(
            "eve@example.com\r\nBCC: victim@example.com",
            "Résumé 更新",
            "hi",
            None,
        );
        // The injected header break must not survive sanitization: the BCC
        // payload survives only as inert text inside the To value, never as
        // its own header line.
        assert!(
            !draft
                .lines()
                .any(|l| l.starts_with("BCC:") || l.starts_with("Cc:"))
        );
        assert!(draft.starts_with("To: eve@example.com BCC: victim@example.com\r\n"));
        // RFC 2047 word; mailparse must decode it back.
        assert!(draft.contains("Subject: =?utf-8?B?"));
        let subject_line = draft
            .lines()
            .find(|l| l.starts_with("Subject: "))
            .expect("subject header");
        let (header, _) = parse_header(subject_line.as_bytes()).expect("parseable header");
        assert_eq!(header.get_value(), "Résumé 更新");
    }

    // -- config parsing -------------------------------------------------------------

    fn base_config() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("imap_host".to_string(), "imap.example.com".to_string()),
            ("imap_port".to_string(), "993".to_string()),
            ("username".to_string(), "me@example.com".to_string()),
            ("password".to_string(), "secret".to_string()),
        ])
    }

    #[test]
    fn parse_config_reads_catalog_field_names() {
        let cfg = parse_config(&base_config()).expect("config");
        assert_eq!(cfg.host, "imap.example.com");
        assert_eq!(cfg.port, 993);
        assert_eq!(cfg.username, "me@example.com");
        assert_eq!(cfg.password, "secret");
        assert_eq!(cfg.drafts_mailbox, "Drafts");
    }

    #[test]
    fn parse_config_requires_all_catalog_fields() {
        for key in ["imap_host", "imap_port", "username", "password"] {
            let mut config = base_config();
            config.remove(key);
            match parse_config(&config) {
                Err(DataSourceError::MissingConfig(field)) => assert_eq!(field, key),
                other => panic!("Expected MissingConfig({key}), got {other:?}"),
            }
        }
    }

    #[test]
    fn parse_config_rejects_non_numeric_port() {
        let mut config = base_config();
        config.insert("imap_port".to_string(), "not-a-port".to_string());
        let err = parse_config(&config).expect_err("invalid port");
        assert!(err.to_string().contains("imap_port"), "got: {err}");
    }
}
