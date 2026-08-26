//! Streaming reader for the session event log.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use shannon_types::session_event::{SessionEvent, SessionEventKind};
use std::str::FromStr;

use super::SessionLogError;

/// Minimal probe used to check a line's `kind` before committing to a full
/// (typed) parse, so unknown kinds produce a precise [`SessionLogError::UnknownEvent`]
/// instead of a generic serde variant error.
#[derive(Deserialize)]
struct KindProbe {
    kind: String,
}

/// Parse one JSONL line into a [`SessionEvent`].
///
/// Unknown kinds are an error (required-by-default semantics); skipping them
/// is the caller's explicit opt-in, handled one level up in the iterator.
fn parse_line(line: &str, line_no: usize) -> Result<SessionEvent, SessionLogError> {
    let probe: KindProbe =
        serde_json::from_str(line).map_err(|e| SessionLogError::MalformedEvent {
            line: line_no,
            message: format!("missing or invalid `kind` field: {e}"),
        })?;
    if SessionEventKind::from_str(&probe.kind).is_err() {
        return Err(SessionLogError::UnknownEvent {
            line: line_no,
            kind: probe.kind,
        });
    }
    serde_json::from_str(line).map_err(|e| SessionLogError::MalformedEvent {
        line: line_no,
        message: e.to_string(),
    })
}

/// Read-only view of a session's event log. Opening never takes the writer's
/// exclusive lock, so reads can proceed while a writer is active (read what
/// has been flushed).
#[derive(Debug, Clone)]
pub struct SessionLogReader {
    path: PathBuf,
}

impl SessionLogReader {
    /// Open the reader for an existing events file.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, SessionLogError> {
        let path = path.into();
        if !path.exists() {
            return Err(SessionLogError::NotFound(path));
        }
        Ok(Self { path })
    }

    /// The events file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Stream all events, one line at a time.
    ///
    /// `skip_unknown_kinds` is the explicit opt-in from plan §4.1: when
    /// false (default), an unknown kind fails with
    /// [`SessionLogError::UnknownEvent`]; when true, such lines are skipped.
    ///
    /// Fails if the file disappeared between [`Self::open`] and now.
    pub fn events(&self, skip_unknown_kinds: bool) -> Result<SessionEventIter, SessionLogError> {
        SessionEventIter::new(&self.path, skip_unknown_kinds)
    }

    /// Materialize all events into a vector. See [`Self::events`] for the
    /// unknown-kind policy.
    pub fn read_events(
        &self,
        skip_unknown_kinds: bool,
    ) -> Result<Vec<SessionEvent>, SessionLogError> {
        let mut events = Vec::new();
        for item in self.events(skip_unknown_kinds)? {
            events.push(item?);
        }
        Ok(events)
    }
}

/// Streaming iterator over a session log, line by line.
///
/// - Blank lines are tolerated (skipped).
/// - A trailing fragment without a final newline is ignored: it is the
///   crash-window garbage that [`super::SessionLogWriter`] repairs on open.
/// - A malformed line (invalid JSON, invalid UTF-8, or payload not matching
///   the declared kind) always fails — data is never silently dropped.
pub struct SessionEventIter {
    reader: BufReader<File>,
    path: PathBuf,
    buf: Vec<u8>,
    line_no: usize,
    skip_unknown: bool,
    done: bool,
}

impl SessionEventIter {
    fn new(path: &Path, skip_unknown: bool) -> Result<Self, SessionLogError> {
        let file = File::open(path)?;
        Ok(Self {
            reader: BufReader::new(file),
            path: path.to_path_buf(),
            buf: Vec::new(),
            line_no: 0,
            skip_unknown,
            done: false,
        })
    }
}

impl Iterator for SessionEventIter {
    type Item = Result<SessionEvent, SessionLogError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.done {
                return None;
            }
            self.buf.clear();
            let n = match self.reader.read_until(b'\n', &mut self.buf) {
                Ok(n) => n,
                Err(e) => {
                    self.done = true;
                    return Some(Err(SessionLogError::Io(e)));
                }
            };
            if n == 0 {
                self.done = true;
                return None;
            }
            if self.buf.last() != Some(&b'\n') {
                // Truncated tail fragment: the writer's recovery repairs it;
                // readers treat everything after the last newline as absent.
                tracing::debug!(
                    path = %self.path.display(),
                    "ignoring truncated tail fragment in session log"
                );
                self.done = true;
                return None;
            }
            self.line_no += 1;
            let line = match std::str::from_utf8(&self.buf) {
                Ok(line) => line,
                Err(e) => {
                    self.done = true;
                    return Some(Err(SessionLogError::MalformedEvent {
                        line: self.line_no,
                        message: format!("invalid UTF-8: {e}"),
                    }));
                }
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match parse_line(trimmed, self.line_no) {
                Ok(event) => return Some(Ok(event)),
                Err(SessionLogError::UnknownEvent { .. }) if self.skip_unknown => continue,
                Err(e) => {
                    self.done = true;
                    return Some(Err(e));
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::writer::SessionLogWriter;
    use super::*;
    use shannon_types::session_event::{AssistantChunkPayload, ErrorPayload, SessionEventBody};
    use tempfile::TempDir;

    fn chunk_event(seq: u64) -> SessionEvent {
        SessionEvent::new(
            seq,
            1_000 + seq,
            "sess-read",
            1,
            SessionEventBody::AssistantChunk(AssistantChunkPayload {
                delta: format!("d{seq}"),
                thinking: false,
            }),
        )
    }

    fn write_raw(dir: &TempDir, contents: &str) -> PathBuf {
        let path = dir.path().join("sessions").join("s1").join("events.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn test_reads_writer_output_streaming() {
        let dir = TempDir::new().unwrap();
        let mut writer = SessionLogWriter::open_in_dir(dir.path(), "s1").unwrap();
        writer.record(SessionEventBody::AssistantChunk(AssistantChunkPayload {
            delta: "a".into(),
            thinking: false,
        }));
        writer.record(SessionEventBody::Error(ErrorPayload {
            category: "warning".into(),
            message: "w".into(),
            detail: None,
        }));
        // Explicit flush so the reader sees them without the writer being
        // closed (readers never take the writer's exclusive lock).
        writer.flush().unwrap();

        let reader = SessionLogReader::open(writer.path()).unwrap();
        let events: Vec<_> = reader.events(false).unwrap().map(Result::unwrap).collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[1].seq, 1);
    }

    #[test]
    fn test_unknown_kind_rejected_by_default() {
        let dir = TempDir::new().unwrap();
        let good1 = serde_json::to_string(&chunk_event(0)).unwrap();
        let unknown =
            r#"{"seq":1,"ts_ns":11,"session_id":"s","turn":1,"kind":"plugin/future-thing","x":1}"#;
        let good2 = serde_json::to_string(&chunk_event(2)).unwrap();
        let path = write_raw(&dir, &format!("{good1}\n{unknown}\n{good2}\n"));

        let reader = SessionLogReader::open(&path).unwrap();
        let err = reader.read_events(false).unwrap_err();
        match &err {
            SessionLogError::UnknownEvent { line, kind } => {
                assert_eq!(*line, 2);
                assert_eq!(kind, "plugin/future-thing");
            }
            other => panic!("expected UnknownEvent, got {other:?}"),
        }
    }

    #[test]
    fn test_unknown_kind_skipped_with_explicit_opt_in() {
        let dir = TempDir::new().unwrap();
        let good1 = serde_json::to_string(&chunk_event(0)).unwrap();
        let unknown =
            r#"{"seq":1,"ts_ns":11,"session_id":"s","turn":1,"kind":"plugin/future-thing"}"#;
        let good2 = serde_json::to_string(&chunk_event(2)).unwrap();
        let path = write_raw(&dir, &format!("{good1}\n{unknown}\n{good2}\n"));

        let reader = SessionLogReader::open(&path).unwrap();
        let events = reader.read_events(true).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[1].seq, 2);
    }

    #[test]
    fn test_malformed_json_line_rejected() {
        let dir = TempDir::new().unwrap();
        let path = write_raw(&dir, "not json at all\n");
        let reader = SessionLogReader::open(&path).unwrap();
        match reader.read_events(false) {
            Err(SessionLogError::MalformedEvent { line, .. }) => assert_eq!(line, 1),
            other => panic!("expected MalformedEvent, got {other:?}"),
        }
    }

    #[test]
    fn test_invalid_utf8_rejected() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("sessions").join("s1").join("events.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{\"seq\":0,\"ts_ns\":1,\"session_id\":\"\xff\xfe\",\"turn\":0,\"kind\":\"turn/start\"}\n").unwrap();
        let reader = SessionLogReader::open(&path).unwrap();
        assert!(matches!(
            reader.read_events(false),
            Err(SessionLogError::MalformedEvent { .. })
        ));
    }

    #[test]
    fn test_blank_lines_tolerated() {
        let dir = TempDir::new().unwrap();
        let good = serde_json::to_string(&chunk_event(0)).unwrap();
        let path = write_raw(&dir, &format!("\n{good}\n\n"));
        let reader = SessionLogReader::open(&path).unwrap();
        assert_eq!(reader.read_events(false).unwrap().len(), 1);
    }

    #[test]
    fn test_trailing_fragment_without_newline_ignored() {
        let dir = TempDir::new().unwrap();
        let good = serde_json::to_string(&chunk_event(0)).unwrap();
        let path = write_raw(&dir, &format!("{good}\n{{\"seq\":1,\"kind\":\"tur"));
        let reader = SessionLogReader::open(&path).unwrap();
        assert_eq!(reader.read_events(false).unwrap().len(), 1);
    }

    #[test]
    fn test_missing_file_not_found() {
        let dir = TempDir::new().unwrap();
        let path = dir
            .path()
            .join("sessions")
            .join("nope")
            .join("events.jsonl");
        match SessionLogReader::open(&path) {
            Err(SessionLogError::NotFound(p)) => assert_eq!(p, path),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn test_payload_mismatching_kind_rejected() {
        let dir = TempDir::new().unwrap();
        // Declared kind tool/call but the payload fields belong to turn/start.
        let line =
            r#"{"seq":0,"ts_ns":1,"session_id":"s","turn":0,"kind":"tool/call","query_id":"q"}"#;
        let path = write_raw(&dir, &format!("{line}\n"));
        let reader = SessionLogReader::open(&path).unwrap();
        assert!(matches!(
            reader.read_events(false),
            Err(SessionLogError::MalformedEvent { .. })
        ));
    }
}
