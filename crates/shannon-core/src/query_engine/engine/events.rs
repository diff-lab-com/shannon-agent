//! Query-event channel plumbing (review §P3-6): the bounded [`EventTx`]
//! facade + [`QUERY_EVENT_CHANNEL_CAPACITY`] and the [`AbortOnDropStream`]
//! cancel wrapper. Moved verbatim from the former single-file `engine.rs`.

use super::*;

/// Capacity of the bounded query-event channel created by
/// [`QueryEngine::process_query`] (review §P3-6).
///
/// Backpressure contract: the producer task sends events with
/// [`mpsc::Sender::send`] (`.await`). While a consumer keeps draining — the
/// REPL pump, the SSE handlers, the WS handler, the CLI — the 256-slot buffer
/// is plenty to absorb scheduling jitter. If a consumer stalls, the producer
/// (and every tool streaming progress through it) suspends at its next send
/// instead of letting memory grow without bound. This is flow control, not
/// loss prevention: no event is ever dropped or coalesced, so the number only
/// trades worst-case queued memory (~256 events) against tolerance for a
/// briefly busy consumer; it must not be raised to mask a genuinely stalled
/// consumer.
///
/// 256 is also far above the largest burst a single engine step produces
/// (one turn streams at most a few hundred tokens' worth of events while the
/// consumer is typically draining continuously), and it is large enough that
/// producer-side batching (e.g. multi-input bus batches) never deadlocks:
/// the producer never waits on the consumer for anything except channel
/// space.
pub(crate) const QUERY_EVENT_CHANNEL_CAPACITY: usize = 256;

/// The engine's event sender: the legacy mpsc facade (§4.2 compatibility —
/// TUI/SSE/desktop consumers unchanged) plus the session [`EventBus`] (§4.8).
/// This is still the **single injection point**: every `QueryEvent` broadcast
/// passes through [`EventTx::send`], which publishes the mapped inputs onto
/// the session bus on their way to the consumer. The built-in L0 subscriber
/// ([`L0TeeSubscriber`](crate::session_log::L0TeeSubscriber)) mirrors durable
/// rows into the session log; publishing happens before the channel send, so
/// log order equals broadcast order exactly like the pre-bus direct bypass.
///
/// The wrapped channel is **bounded**
/// ([`QUERY_EVENT_CHANNEL_CAPACITY`]) and [`EventTx::send`] is `async`:
/// a slow consumer suspends the producer at the send (§P3-6 backpressure)
/// while bus dispatch (in-process, synchronous) still happens eagerly.
#[derive(Clone)]
pub(crate) struct EventTx {
    tx: mpsc::Sender<Result<QueryEvent, QueryError>>,
    bus: crate::bus::EventBus,
}

impl EventTx {
    pub(super) fn new(
        tx: mpsc::Sender<Result<QueryEvent, QueryError>>,
        bus: crate::bus::EventBus,
    ) -> Self {
        Self { tx, bus }
    }

    /// Send one event to the consumer, suspending while the bounded buffer is
    /// full (backpressure). Fails iff the consumer dropped the
    /// [`QueryStream`]; the error carries the event back, matching the
    /// previous unbounded-channel `SendError` semantics.
    pub(super) async fn send(
        &self,
        item: Result<QueryEvent, QueryError>,
    ) -> Result<(), mpsc::error::SendError<Result<QueryEvent, QueryError>>> {
        if let Ok(event) = &item {
            // Serial batches keep multi-input expansions (`Failed` → error row
            // + turn boundary) atomic against foreign events.
            self.bus
                .dispatch_serial_batch(crate::session_log::query_event_to_bus_inputs(event));
        }
        self.tx.send(item).await
    }
}

/// Stream wrapper that aborts the spawned producer task when the stream is
/// dropped, so a consumer can cancel an in-progress query simply by dropping
/// the [`QueryStream`].
///
/// Why this is needed: `send_event!` only logs a "receiver closed" warning and
/// keeps running, so dropping the receiver alone would leave the LLM/tool loop
/// producing tokens and executing tools after the client cancelled. Capturing
/// the producer's `JoinHandle` here and calling `abort()` on drop cancels the
/// task at its next `.await` point (e.g. between streamed tokens, or at the
/// next tool-call boundary).
pub(super) struct AbortOnDropStream<S> {
    inner: std::pin::Pin<Box<S>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl<S> AbortOnDropStream<S> {
    pub(super) fn new(stream: S, handle: tokio::task::JoinHandle<()>) -> Self {
        Self {
            inner: Box::pin(stream),
            handle: Some(handle),
        }
    }
}

impl<S> Drop for AbortOnDropStream<S> {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

impl<S: futures::Stream> futures::Stream for AbortOnDropStream<S> {
    type Item = S::Item;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        // Both fields are `Unpin`, so the wrapper itself is `Unpin` and we can
        // project safely onto the inner stream.
        let this = self.get_mut();
        this.inner.as_mut().poll_next(cx)
    }
}
