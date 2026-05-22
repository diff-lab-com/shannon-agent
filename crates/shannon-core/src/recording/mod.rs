//! Session recording and replay infrastructure.
//!
//! Records the full query lifecycle (LLM requests/responses, tool calls,
//! query events) as JSONL for deterministic replay testing.

pub(crate) mod types;
mod recorder;
mod replayer;
mod fixture;

pub use types::{RecordingEntry, SessionRecordingMeta};
pub use recorder::SessionRecorder;
pub use replayer::SessionReplayer;
pub use fixture::ToolChainTest;
