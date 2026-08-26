//! The single-writer append-only session log writer.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde_json::json;
use shannon_types::session_event::{
    ErrorPayload, SessionEvent, SessionEventBody, SessionEventKind,
};
use tracing::warn;

use super::{SessionLogError, session_events_path};

/// Aggregate this many `assistant/chunk` events before flushing.
pub const DEFAULT_CHUNK_FLUSH_COUNT: usize = 50;
/// Or flush chunk aggregation once this much time has passed since the last
/// flush. Checked lazily on write (no background timer).
pub const DEFAULT_CHUNK_FLUSH_INTERVAL: Duration = Duration::from_millis(50);

/// When the buffered writer drains to the OS. `assistant/chunk` events are
/// aggregated; `tool/result`, `turn/end`, and `request/header` events force
/// an immediate flush (they mark replay-safe boundaries).
#[derive(Debug, Clone)]
pub struct FlushPolicy {
    /// Flush after this many buffered chunk events.
    pub chunk_count: usize,
    /// Flush when this much time elapsed since the last flush.
    pub chunk_interval: Duration,
}

impl Default for FlushPolicy {
    fn default() -> Self {
        Self {
            chunk_count: DEFAULT_CHUNK_FLUSH_COUNT,
            chunk_interval: DEFAULT_CHUNK_FLUSH_INTERVAL,
        }
    }
}

/// Result of scanning an existing log file for tail recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TailScan {
    /// Number of complete lines (each consumed one seq slot).
    complete_lines: u64,
    /// File offset just past the last complete line.
    complete_bytes: u64,
    /// Bytes of a trailing partial line (crash-window garbage), if any.
    trailing_bytes: u64,
    /// Turn number of the last complete event, so the envelope turn counter
    /// resumes across reopens.
    last_turn: Option<u64>,
}

/// The exclusive append-only writer for one session's event log.
///
/// Invariants:
///
/// - **Single writer**: opening takes an exclusive `flock` on the events
///   file. A second writer on the same file is rejected with
///   [`SessionLogError::AlreadyLocked`].
/// - **seq = events written**: [`SessionEvent::seq`] is assigned here and
///   equals the number of events already in the file, continuing across
///   reopens. A failed write does not consume a seq (the event is simply
///   absent and counted in `failures`), so seqs stay strictly continuous.
///   The envelope turn counter also resumes from the last complete event.
/// - **Never fails the session**: [`SessionLogWriter::record`] is infallible.
///   I/O failures degrade to a failure counter plus a `warn!` log.
/// - **Tail recovery**: opening a log whose last line was truncated (crash
///   mid-write) truncates back to the last complete line and appends an
///   `error` event (`log-corruption`) describing the repair.
pub struct SessionLogWriter {
    out: BufWriter<File>,
    path: PathBuf,
    session_id: String,
    /// Seq the next successfully written event will get.
    next_seq: u64,
    current_turn: u64,
    span_id: Option<String>,
    parent_span_id: Option<String>,
    chunk_since_flush: usize,
    last_flush: Instant,
    policy: FlushPolicy,
    /// Write/flush failures since open (degraded-mode counter).
    failures: u64,
    /// Test seam: force the next write attempt to fail once.
    #[cfg(test)]
    fail_next_write: bool,
}

impl SessionLogWriter {
    /// Open (or resume) `~/.shannon/sessions/<session_id>/events.jsonl`,
    /// honoring the `SHANNON_HOME` override.
    pub fn open(session_id: &str) -> Result<Self, SessionLogError> {
        let home = super::default_shannon_home()?;
        Self::open_in_dir(&home, session_id)
    }

    /// Open (or resume) `<base_dir>/sessions/<session_id>/events.jsonl`.
    pub fn open_in_dir(base_dir: &Path, session_id: &str) -> Result<Self, SessionLogError> {
        let path = session_events_path(base_dir, session_id);
        Self::open_path(path, session_id)
    }

    fn open_path(path: PathBuf, session_id: &str) -> Result<Self, SessionLogError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // One handle, opened read+append+create. The read right lets the
        // recovery scan share this handle's lifetime; the flock below makes
        // the ownership of the file exclusive to this writer.
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)?;
        // Fail fast when another writer holds the log (plan §4.1 ④).
        FileExt::try_lock_exclusive(&file).map_err(|source| SessionLogError::AlreadyLocked {
            path: path.clone(),
            source,
        })?;

        // Tail recovery while we hold the exclusive lock (plan §4.1 ③).
        let scan = scan_tail(&path)?;
        if scan.trailing_bytes > 0 {
            file.set_len(scan.complete_bytes)?;
        }

        let mut writer = Self {
            out: BufWriter::new(file),
            path,
            session_id: session_id.to_string(),
            next_seq: scan.complete_lines,
            current_turn: scan.last_turn.unwrap_or(0),
            span_id: None,
            parent_span_id: None,
            chunk_since_flush: 0,
            last_flush: Instant::now(),
            policy: FlushPolicy::default(),
            failures: 0,
            #[cfg(test)]
            fail_next_write: false,
        };

        if scan.trailing_bytes > 0 {
            // The repair itself goes through the normal write path so it
            // gets a proper seq and flush.
            writer.record(SessionEventBody::Error(ErrorPayload {
                category: ErrorPayload::CATEGORY_LOG_CORRUPTION.into(),
                message: format!(
                    "session log had a truncated tail line; recovered by truncating {} bytes \
                     back to the last complete line",
                    scan.trailing_bytes
                ),
                detail: Some(json!({ "dropped_bytes": scan.trailing_bytes })),
            }));
        }
        Ok(writer)
    }

    /// Record one event. **Infallible by design**: on failure the event is
    /// dropped, counted, and warned about — a log problem must never fail
    /// the session (plan §4.1 constraint).
    ///
    /// Returns the seq assigned to the event. On a dropped write that seq is
    /// *not* consumed; the next successful event reuses it.
    ///
    /// `turn/start` events advance the envelope turn counter; other
    /// turn/span state comes from [`set_turn`](Self::set_turn) /
    /// [`set_span`](Self::set_span).
    pub fn record(&mut self, body: SessionEventBody) -> u64 {
        if matches!(body, SessionEventBody::TurnStart(_)) {
            self.current_turn += 1;
        }
        let kind = body.kind();
        let event = SessionEvent {
            seq: self.next_seq,
            ts_ns: now_ts_ns(),
            session_id: self.session_id.clone(),
            turn: self.current_turn,
            step: None,
            span_id: self.span_id.clone(),
            parent_span_id: self.parent_span_id.clone(),
            body,
        };
        match self.write_event(&event) {
            Ok(()) => {
                self.next_seq += 1;
                self.maybe_flush(kind);
            }
            Err(e) => {
                self.failures += 1;
                warn!(
                    path = %self.path.display(),
                    session_id = %self.session_id,
                    error = %e,
                    failures = self.failures,
                    "session log write failed; event dropped, seq not consumed"
                );
            }
        }
        event.seq
    }

    fn write_event(&mut self, event: &SessionEvent) -> Result<(), SessionLogError> {
        #[cfg(test)]
        if self.fail_next_write {
            self.fail_next_write = false;
            return Err(SessionLogError::Io(std::io::Error::other(
                "simulated write failure",
            )));
        }
        let line = serde_json::to_string(event)
            .map_err(|e| SessionLogError::Serialization(e.to_string()))?;
        self.out
            .write_all(line.as_bytes())
            .and_then(|()| self.out.write_all(b"\n"))
            .map_err(SessionLogError::Io)
    }

    /// Apply the flush policy after a successful write.
    fn maybe_flush(&mut self, kind: SessionEventKind) {
        if kind == SessionEventKind::AssistantChunk {
            self.chunk_since_flush += 1;
        }
        let force = matches!(
            kind,
            SessionEventKind::ToolResult
                | SessionEventKind::TurnEnd
                | SessionEventKind::RequestHeader
        );
        let chunk_due = self.chunk_since_flush >= self.policy.chunk_count
            || self.last_flush.elapsed() >= self.policy.chunk_interval;
        if force || (kind == SessionEventKind::AssistantChunk && chunk_due) {
            self.do_flush();
        }
    }

    /// Flush, counting + warning on failure instead of propagating it (a
    /// flush failure must not kill the session either).
    fn do_flush(&mut self) {
        if let Err(e) = self.out.flush() {
            self.failures += 1;
            warn!(
                path = %self.path.display(),
                error = %e,
                failures = self.failures,
                "session log flush failed"
            );
        }
        self.chunk_since_flush = 0;
        self.last_flush = Instant::now();
    }

    /// Explicitly flush buffered events, surfacing the error.
    pub fn flush(&mut self) -> Result<(), SessionLogError> {
        self.out.flush()?;
        self.chunk_since_flush = 0;
        self.last_flush = Instant::now();
        Ok(())
    }

    /// Flush and close the log, surfacing the final flush error that the
    /// `Drop` of `BufWriter` would otherwise swallow.
    pub fn close(mut self) -> Result<(), SessionLogError> {
        self.flush()
    }

    /// Override the flush policy (defaults: 50 chunks / 50ms).
    pub fn set_flush_policy(&mut self, policy: FlushPolicy) {
        self.policy = policy;
    }

    /// Set the envelope turn explicitly (otherwise advanced by `turn/start`).
    pub fn set_turn(&mut self, turn: u64) {
        self.current_turn = turn;
    }

    /// Set the span context applied to subsequent events.
    pub fn set_span(&mut self, span_id: Option<String>, parent_span_id: Option<String>) {
        self.span_id = span_id;
        self.parent_span_id = parent_span_id;
    }

    /// Seq the next successfully written event will receive
    /// (== events written so far).
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// The current envelope turn number.
    pub fn current_turn(&self) -> u64 {
        self.current_turn
    }

    /// Write/flush failures since open (degraded-mode counter).
    pub fn failures(&self) -> u64 {
        self.failures
    }

    /// True once any write/flush failure occurred.
    pub fn is_degraded(&self) -> bool {
        self.failures > 0
    }

    /// Path of the events file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Session id this log belongs to.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Test seam: make the next write attempt fail exactly once, exercising
    /// the degraded path (count + warn + seq reuse).
    #[cfg(test)]
    pub(crate) fn simulate_write_failure(&mut self) {
        self.fail_next_write = true;
    }
}

/// Nanoseconds since the Unix epoch for event timestamps.
fn now_ts_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        // A clock set before 1970 is not a usable environment; 0 keeps the
        // log valid rather than panicking inside the never-fail path.
        .unwrap_or(0)
}

/// Scan a log file, counting complete lines and detecting a trailing partial
/// line (a crash-window write that never finished). Returns zeros for a
/// missing file (fresh session).
fn scan_tail(path: &Path) -> Result<TailScan, SessionLogError> {
    if !path.exists() {
        return Ok(TailScan {
            complete_lines: 0,
            complete_bytes: 0,
            trailing_bytes: 0,
            last_turn: None,
        });
    }
    let file = File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let mut buf = Vec::new();
    let mut last_complete_line: Vec<u8> = Vec::new();
    let mut complete_lines = 0u64;
    let mut complete_bytes = 0u64;
    let mut trailing_bytes = 0u64;
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        if buf.last() == Some(&b'\n') {
            complete_lines += 1;
            complete_bytes += n as u64;
            last_complete_line.clear();
            last_complete_line.extend_from_slice(&buf);
        } else {
            // Only the final read can lack the newline: partial tail line.
            trailing_bytes = n as u64;
        }
    }
    // Best-effort: resume the turn counter from the last complete event.
    // A line that fails to parse here leaves the counter at 0; the reader
    // will surface the corruption precisely.
    let last_turn = std::str::from_utf8(&last_complete_line)
        .ok()
        .and_then(|line| serde_json::from_str::<SessionEvent>(line).ok())
        .map(|event| event.turn);
    Ok(TailScan {
        complete_lines,
        complete_bytes,
        trailing_bytes,
        last_turn,
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::super::reader::SessionLogReader;
    use super::*;
    use shannon_types::session_event::{
        AssistantChunkPayload, ToolResultPayload, TurnStartPayload, UserMessagePayload,
    };
    use tempfile::TempDir;

    fn chunk(delta: &str) -> SessionEventBody {
        SessionEventBody::AssistantChunk(AssistantChunkPayload {
            delta: delta.into(),
            thinking: false,
        })
    }

    fn tool_result() -> SessionEventBody {
        SessionEventBody::ToolResult(ToolResultPayload {
            tool_use_id: "t1".into(),
            tool_name: "Bash".into(),
            output: "ok".into(),
            is_error: false,
            duration_ms: Some(1),
            meta: serde_json::Value::Null,
        })
    }

    #[test]
    fn test_write_and_read_back_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let mut writer = SessionLogWriter::open_in_dir(dir.path(), "sess-1").expect("open writer");
        assert_eq!(writer.next_seq(), 0);
        writer.record(chunk("he"));
        writer.record(chunk("llo"));
        writer.record(tool_result());
        let path = writer.path().to_path_buf();
        writer.close().expect("close");

        let reader = SessionLogReader::open(&path).expect("open reader");
        let events = reader.read_events(false).expect("read events");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[1].seq, 1);
        assert_eq!(events[2].seq, 2);
        assert_eq!(events[2].kind(), SessionEventKind::ToolResult);
        assert_eq!(events[0].session_id, "sess-1");
    }

    /// Verification standard ④: a second writer on the same file is
    /// rejected (exclusive lock).
    #[test]
    fn test_second_writer_rejected() {
        let dir = TempDir::new().expect("tempdir");
        let _first = SessionLogWriter::open_in_dir(dir.path(), "sess-lock").expect("first");
        let second = SessionLogWriter::open_in_dir(dir.path(), "sess-lock");
        match second {
            Err(SessionLogError::AlreadyLocked { .. }) => {}
            Err(other) => panic!("expected AlreadyLocked, got {other}"),
            Ok(_) => panic!("second writer unexpectedly acquired the log"),
        }
        // A different session file is unaffected.
        assert!(SessionLogWriter::open_in_dir(dir.path(), "sess-other").is_ok());
    }

    /// Verification standard ③: a truncated tail is repaired and an error
    /// event records the repair.
    #[test]
    fn test_truncated_tail_recovered() {
        let dir = TempDir::new().expect("tempdir");
        let session_id = "sess-trunc";
        {
            let mut writer =
                SessionLogWriter::open_in_dir(dir.path(), session_id).expect("open writer");
            for i in 0..100 {
                writer.record(chunk(&format!("c{i}")));
            }
            writer.close().expect("close");
        }
        // Simulate a crash mid-write: append half a line.
        let path = session_events_path(dir.path(), session_id);
        let half_line: &[u8] = br#"{"seq":100,"ts_ns":123,"session_id":"sess-trunc"#;
        let mut file = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open append");
        file.write_all(half_line).expect("write half line");

        // Reopen: recovers and appends a warning event.
        let mut writer = SessionLogWriter::open_in_dir(dir.path(), session_id).expect("reopen");
        // The warning event itself consumed seq 100 during open.
        assert_eq!(writer.next_seq(), 101);
        writer.record(chunk("post-recovery"));
        writer.close().expect("close");

        let reader = SessionLogReader::open(&path).expect("open reader");
        let events = reader.read_events(false).expect("read events");
        // 100 written + 1 corruption warning + 1 post-recovery chunk.
        assert_eq!(events.len(), 102);
        let warning = &events[100];
        assert_eq!(warning.kind(), SessionEventKind::Error);
        match &warning.body {
            SessionEventBody::Error(e) => {
                assert_eq!(e.category, ErrorPayload::CATEGORY_LOG_CORRUPTION);
                let detail = e.detail.as_ref().expect("repair event carries detail");
                assert_eq!(detail["dropped_bytes"], json!(half_line.len()));
            }
            other => panic!("wrong body: {other:?}"),
        }
        // Seq continuity holds across the repair.
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.seq, i as u64);
        }
        assert_eq!(events[101].seq, 101);
    }

    /// Verification standard ②: 10k pseudo-random interleaved appends (with
    /// reopen cycles) read back with strictly continuous seqs.
    #[test]
    fn test_fuzz_interleaved_appends_seq_strictly_continuous() {
        // Deterministic xorshift so failures reproduce.
        let mut rng_state: u64 = 0x9E3779B97F4A7C15;
        let mut rand = move || {
            rng_state ^= rng_state << 13;
            rng_state ^= rng_state >> 7;
            rng_state ^= rng_state << 17;
            rng_state
        };

        let dir = TempDir::new().expect("tempdir");
        let session_id = "sess-fuzz";
        let mut total_written = 0u64;

        // Batches of 997 interleave appends with close/reopen cycles, which
        // exercise tail recovery and seq resumption along the way.
        while total_written < 10_000 {
            let batch = 997.min(10_000 - total_written);
            let mut writer =
                SessionLogWriter::open_in_dir(dir.path(), session_id).expect("open writer");
            for _ in 0..batch {
                let body = match rand() % 6 {
                    0 => chunk("delta"),
                    1 => tool_result(),
                    2 => SessionEventBody::TurnStart(TurnStartPayload { query_id: None }),
                    3 => SessionEventBody::UserMessage(UserMessagePayload {
                        source: UserMessagePayload::SOURCE_USER.into(),
                        content: "msg".into(),
                    }),
                    4 => SessionEventBody::Error(ErrorPayload {
                        category: "warning".into(),
                        message: "w".into(),
                        detail: None,
                    }),
                    _ => tool_result(),
                };
                writer.record(body);
                total_written += 1;
            }
            writer.close().expect("close");
        }
        assert_eq!(total_written, 10_000);

        let path = session_events_path(dir.path(), session_id);
        let reader = SessionLogReader::open(&path).expect("open reader");
        let events = reader.read_events(false).expect("read events");
        assert_eq!(events.len() as u64, total_written);
        for (i, event) in events.iter().enumerate() {
            assert_eq!(
                event.seq,
                i as u64,
                "seq gap/dup at index {i} (kind {})",
                event.kind()
            );
        }
    }

    #[test]
    fn test_tool_result_forces_flush_to_disk() {
        let dir = TempDir::new().expect("tempdir");
        let mut writer =
            SessionLogWriter::open_in_dir(dir.path(), "sess-flush").expect("open writer");
        // Chunks aggregate; without the forced flush they may still sit in
        // the BufWriter, but the tool/result must be on disk immediately.
        writer.set_flush_policy(FlushPolicy {
            chunk_count: 10_000,
            chunk_interval: Duration::from_secs(3_600),
        });
        for i in 0..20 {
            writer.record(chunk(&format!("c{i}")));
        }
        let before = std::fs::read_to_string(writer.path()).expect("read file");
        writer.record(tool_result());
        let after = std::fs::read_to_string(writer.path()).expect("read file");
        assert!(
            after.contains("\"kind\":\"tool/result\""),
            "tool/result must be flushed synchronously"
        );
        assert!(after.len() > before.len());
    }

    #[test]
    fn test_chunk_count_threshold_triggers_flush() {
        let dir = TempDir::new().expect("tempdir");
        let mut writer =
            SessionLogWriter::open_in_dir(dir.path(), "sess-chunks").expect("open writer");
        // Deterministic thresholds: 3 chunks or 1 hour.
        writer.set_flush_policy(FlushPolicy {
            chunk_count: 3,
            chunk_interval: Duration::from_secs(3_600),
        });
        writer.record(chunk("1"));
        writer.record(chunk("2"));
        // Below threshold: the events are still buffered.
        let pending = std::fs::read_to_string(writer.path()).expect("read file");
        assert!(!pending.contains("\"kind\":\"assistant/chunk\""));
        // Third chunk crosses the threshold.
        writer.record(chunk("3"));
        let flushed = std::fs::read_to_string(writer.path()).expect("read file");
        assert_eq!(flushed.matches("\"kind\":\"assistant/chunk\"").count(), 3);
    }

    #[test]
    fn test_chunk_interval_triggers_flush() {
        let dir = TempDir::new().expect("tempdir");
        let mut writer =
            SessionLogWriter::open_in_dir(dir.path(), "sess-interval").expect("open writer");
        // Zero interval: the very first chunk flushes on its lazy timer.
        writer.set_flush_policy(FlushPolicy {
            chunk_count: 1_000,
            chunk_interval: Duration::ZERO,
        });
        writer.record(chunk("solo"));
        let on_disk = std::fs::read_to_string(writer.path()).expect("read file");
        assert!(on_disk.contains("\"kind\":\"assistant/chunk\""));
    }

    #[test]
    fn test_turn_start_advances_turn_counter() {
        let dir = TempDir::new().expect("tempdir");
        let mut writer = SessionLogWriter::open_in_dir(dir.path(), "sess-turn").expect("open");
        assert_eq!(writer.current_turn(), 0);
        writer.record(SessionEventBody::TurnStart(TurnStartPayload {
            query_id: None,
        }));
        writer.record(chunk("a"));
        writer.record(SessionEventBody::TurnStart(TurnStartPayload {
            query_id: None,
        }));
        writer.record(chunk("b"));
        let path = writer.path().to_path_buf();
        writer.close().expect("close");

        let reader = SessionLogReader::open(&path).expect("open reader");
        let events = reader.read_events(false).expect("read");
        let turns: Vec<u64> = events.iter().map(|e| e.turn).collect();
        assert_eq!(turns, vec![1, 1, 2, 2]);

        // Explicit set_turn overrides the counter.
        let mut writer = SessionLogWriter::open_in_dir(dir.path(), "sess-turn").expect("reopen");
        assert_eq!(writer.current_turn(), 2, "turn state resumes at last value");
        writer.set_turn(9);
        assert_eq!(writer.current_turn(), 9);
    }

    #[test]
    fn test_span_context_stamped_on_events() {
        let dir = TempDir::new().expect("tempdir");
        let mut writer = SessionLogWriter::open_in_dir(dir.path(), "sess-span").expect("open");
        writer.set_span(Some("span-9".into()), Some("span-1".into()));
        writer.record(chunk("x"));
        writer.set_span(None, None);
        writer.record(chunk("y"));
        let path = writer.path().to_path_buf();
        writer.close().expect("close");

        let reader = SessionLogReader::open(&path).expect("open reader");
        let events = reader.read_events(false).expect("read");
        assert_eq!(events[0].span_id.as_deref(), Some("span-9"));
        assert_eq!(events[0].parent_span_id.as_deref(), Some("span-1"));
        assert_eq!(events[1].span_id, None);
        assert_eq!(events[1].parent_span_id, None);
    }

    #[test]
    fn test_write_failure_degrades_not_crashes() {
        let dir = TempDir::new().expect("tempdir");
        let mut writer = SessionLogWriter::open_in_dir(dir.path(), "sess-degraded").expect("open");
        writer.record(chunk("ok"));
        writer.simulate_write_failure();
        let dropped_seq = writer.record(chunk("dropped"));
        // The dropped event's seq is not consumed…
        assert_eq!(dropped_seq, 1);
        assert_eq!(writer.next_seq(), 1);
        assert_eq!(writer.failures(), 1);
        assert!(writer.is_degraded());
        // …and the next successful event reuses it, keeping seq continuous.
        writer.record(chunk("recovered"));
        assert_eq!(writer.next_seq(), 2);
        let path = writer.path().to_path_buf();
        writer.close().expect("close");

        let reader = SessionLogReader::open(&path).expect("open reader");
        let events = reader.read_events(false).expect("read");
        let deltas: Vec<&str> = events
            .iter()
            .filter_map(|e| match &e.body {
                SessionEventBody::AssistantChunk(c) => Some(c.delta.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas, vec!["ok", "recovered"]);
        assert_eq!(events[1].seq, 1);
    }

    #[test]
    fn test_reopen_resumes_seq_and_creates_directory() {
        let dir = TempDir::new().expect("tempdir");
        {
            let mut writer =
                SessionLogWriter::open_in_dir(dir.path(), "nested/sess-id").expect("open");
            writer.record(chunk("first"));
            writer.close().expect("close");
        }
        {
            let mut writer =
                SessionLogWriter::open_in_dir(dir.path(), "nested/sess-id").expect("reopen");
            assert_eq!(writer.next_seq(), 1);
            writer.record(chunk("second"));
            writer.close().expect("close");
        }
        let path = session_events_path(dir.path(), "nested/sess-id");
        assert!(path.exists());
        let reader = SessionLogReader::open(&path).expect("open reader");
        let events = reader.read_events(false).expect("read");
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].seq, 1);
    }

    #[test]
    fn test_default_flush_policy_matches_plan() {
        let policy = FlushPolicy::default();
        assert_eq!(policy.chunk_count, 50);
        assert_eq!(policy.chunk_interval, Duration::from_millis(50));
    }
}
