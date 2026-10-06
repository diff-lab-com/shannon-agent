use super::*;

#[tokio::test]
async fn abort_on_drop_stream_forwards_items_and_aborts_producer_on_drop() {
    // The whole point of P0-c: dropping the QueryStream must actually stop
    // the engine's producer task, not just close the channel (send_event!
    // only logs a closed receiver and keeps running). AbortOnDropStream
    // guarantees that by calling JoinHandle::abort() on drop.
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    let counter = Arc::new(AtomicU64::new(0));
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<u64>();

    let producer_counter = counter.clone();
    let handle = tokio::spawn(async move {
        let mut n = 0u64;
        loop {
            n += 1;
            producer_counter.fetch_add(1, Ordering::SeqCst);
            // Deliberately ignore send errors — without abort this loop
            // would keep spinning forever after the receiver drops.
            let _ = tx.send(n);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    // Wrap an unfold stream (same shape process_query returns) + the handle.
    use futures::StreamExt as _;
    use futures::stream::unfold;
    let inner = unfold(rx, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut stream = AbortOnDropStream::new(inner, handle);

    // The wrapper forwards items from the inner stream.
    assert!(
        stream.next().await.is_some(),
        "wrapper must forward inner stream items"
    );

    let baseline = counter.load(Ordering::SeqCst);
    // Dropping the wrapper must abort the producer task.
    drop(stream);

    // If the producer were still alive it would tick ~every 5ms; after
    // 50ms that would be ~10 more iterations. Abortion cancels it at its
    // next await, so the counter stays essentially at the baseline.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let after = counter.load(Ordering::SeqCst);
    assert!(
        after <= baseline + 1,
        "producer kept running after stream drop: baseline={baseline} after={after}"
    );
}

// ── Bounded query-event channel backpressure (review §P3-6) ─────────

/// A stalled consumer must suspend the producer (true backpressure), and
/// resuming the consumer must deliver every event, in order, none lost.
#[tokio::test]
async fn bounded_event_channel_producer_suspends_when_consumer_stalls() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    const CAP: usize = 4;
    const TOTAL: usize = 20;
    let query_id = Uuid::new_v4();
    let (tx_raw, mut rx) = mpsc::channel::<Result<QueryEvent, QueryError>>(CAP);
    let tx = EventTx::new(tx_raw, crate::bus::EventBus::new());

    let sent = Arc::new(AtomicUsize::new(0));
    let producer = {
        let sent = sent.clone();
        tokio::spawn(async move {
            for i in 0..TOTAL {
                tx.send(Ok(QueryEvent::Text {
                    query_id,
                    content: format!("e{i}"),
                }))
                .await
                .expect("consumer stays alive");
                sent.fetch_add(1, Ordering::SeqCst);
            }
        })
    };

    // The consumer deliberately does NOT drain. With no recv, at most
    // `CAP` sends can complete — the (CAP+1)-th send must stay pending.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let stalled_sent = sent.load(Ordering::SeqCst);
    assert!(
        stalled_sent <= CAP,
        "producer completed {stalled_sent} sends into a capacity-{CAP} \
         channel that was never drained — backpressure is broken"
    );
    assert!(
        stalled_sent < TOTAL,
        "producer finished all {TOTAL} sends while the consumer was stalled"
    );

    // Resume the consumer (slowly): every event arrives, in order.
    let mut received: Vec<QueryEvent> = Vec::with_capacity(TOTAL);
    for _ in 0..TOTAL {
        let item = rx
            .recv()
            .await
            .expect("channel stays open while the producer is alive");
        received.push(item.expect("no error events sent"));
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    producer.await.expect("producer task finishes");

    assert_eq!(received.len(), TOTAL, "no event may be lost");
    for (i, event) in received.iter().enumerate() {
        match event {
            QueryEvent::Text { content, .. } => {
                assert_eq!(content, &format!("e{i}"), "FIFO order must hold")
            }
            other => panic!("unexpected event in stream: {other:?}"),
        }
    }
}

/// A consumer dropped mid-stream must surface a send error to the
/// producer (bounded channel keeps the unbounded channel's SendError
/// semantics), instead of hanging or silently succeeding.
#[tokio::test]
async fn bounded_event_channel_send_fails_when_consumer_dropped_mid_stream() {
    use std::time::Duration;

    let query_id = Uuid::new_v4();
    let (tx_raw, mut rx) = mpsc::channel::<Result<QueryEvent, QueryError>>(2);
    let tx = EventTx::new(tx_raw, crate::bus::EventBus::new());

    let handle = tokio::spawn(async move {
        for i in 0..64 {
            if tx
                .send(Ok(QueryEvent::Text {
                    query_id,
                    content: format!("e{i}"),
                }))
                .await
                .is_err()
            {
                return Some(i);
            }
        }
        None
    });

    // Consume one event like a real stream consumer, then drop the
    // receiver while the producer is still producing.
    let first = rx.recv().await.expect("at least one event is available");
    assert!(matches!(first, Ok(QueryEvent::Text { .. })));
    drop(rx);

    let failed_at = tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("producer must terminate once the consumer is dropped")
        .expect("producer task does not panic");
    assert!(
        failed_at.is_some(),
        "producer never observed the closed channel"
    );
    // At most `capacity + 1` sends can succeed without a drain, so the
    // failure must surface almost immediately (index ≤ 3 here).
    assert!(
        failed_at.unwrap() <= 3,
        "send error surfaced late — the producer buffered past capacity"
    );
}

/// Regression for the AbortOnDrop contract over a **bounded** channel:
/// dropping the stream still aborts the producer, and the producer cannot
/// keep producing after the drop.
#[tokio::test]
async fn bounded_channel_abort_on_drop_stops_producer() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    let counter = Arc::new(AtomicU64::new(0));
    let query_id = Uuid::new_v4();
    let (tx_raw, rx) = mpsc::channel::<Result<QueryEvent, QueryError>>(2);
    let tx = EventTx::new(tx_raw, crate::bus::EventBus::new());

    let producer_counter = counter.clone();
    let handle = tokio::spawn(async move {
        let mut n = 0u64;
        loop {
            n += 1;
            producer_counter.fetch_add(1, Ordering::SeqCst);
            // Deliberately ignore send errors — without abort, the loop
            // would only end by itself after the receiver drops.
            let _ = tx
                .send(Ok(QueryEvent::Text {
                    query_id,
                    content: format!("e{n}"),
                }))
                .await;
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });

    // Same stream shape process_query returns: unfold + AbortOnDropStream.
    use futures::StreamExt as _;
    use futures::stream::unfold;
    let inner = unfold(rx, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut stream = AbortOnDropStream::new(inner, handle);

    assert!(
        stream.next().await.is_some(),
        "wrapper must forward inner stream items"
    );

    let baseline = counter.load(Ordering::SeqCst);
    drop(stream);

    // An alive producer would tick ~every 5ms; after 50ms that is ~10
    // more iterations. Abortion (or the send error) stops it within one
    // iteration of the drop.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let after = counter.load(Ordering::SeqCst);
    assert!(
        after <= baseline + 1,
        "producer kept running after bounded-channel stream drop: \
         baseline={baseline} after={after}"
    );
}
