//! Session recording and replay infrastructure.
//!
//! Records the full query lifecycle (LLM requests/responses, tool calls,
//! query events) as JSONL for deterministic replay testing.

mod fixture;
mod recorder;
mod replayer;
pub(crate) mod types;

pub use fixture::ToolChainTest;
pub use recorder::SessionRecorder;
pub use replayer::SessionReplayer;
pub use types::{RecordingEntry, SessionRecordingMeta};
