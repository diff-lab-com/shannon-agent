//! §4.8 dispatch-latency benchmark.
//!
//! Plan requirement: dispatch cost per event must stay in the "<100µs"
//! class. Groups measure the fan-out modes with 0/1/8 subscribers plus a
//! two-node guard-pipeline run, so the numbers cover both the engine's
//! L0-mirror path (Serial, single wildcard subscriber) and future guard
//! chains.

#![allow(clippy::unwrap_used)]

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use shannon_core::bus::{BusEvent, BusInput, DispatchMode, EventBus, Flow, TopicFilter};
use shannon_types::session_event::{ErrorPayload, SessionEventBody, SessionEventKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn make_event(tag: usize) -> BusInput {
    BusEvent::new(SessionEventBody::Error(ErrorPayload {
        category: "bench".into(),
        message: format!("payload {tag}"),
        detail: None,
    }))
    .into()
}

struct Sink(AtomicUsize);

impl shannon_core::bus::BusSubscriber for Sink {
    fn on_input(&self, _input: &BusInput) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn bench_emit(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_bus/emit");
    for subscribers in [0usize, 1, 8] {
        group.bench_with_input(
            format!("{subscribers}_subscribers"),
            &subscribers,
            |b, &n| {
                let bus = EventBus::new();
                let sink = Arc::new(Sink(AtomicUsize::new(0)));
                let guards: Vec<_> = (0..n)
                    .map(|_| bus.subscribe(TopicFilter::all(), sink.clone()))
                    .collect();
                let mut tag = 0usize;
                b.iter(|| {
                    tag = tag.wrapping_add(1);
                    bus.dispatch(make_event(tag), DispatchMode::Emit);
                });
                drop(guards);
                if n > 0 {
                    assert!(sink.0.load(Ordering::Relaxed) > 0);
                }
            },
        );
    }
    group.finish();
}

fn bench_serial(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_bus/serial");
    for subscribers in [1usize, 8] {
        group.bench_with_input(
            format!("{subscribers}_subscribers"),
            &subscribers,
            |b, &n| {
                let bus = EventBus::new();
                let sink = Arc::new(Sink(AtomicUsize::new(0)));
                let guards: Vec<_> = (0..n)
                    .map(|_| bus.subscribe(TopicFilter::all(), sink.clone()))
                    .collect();
                let mut tag = 0usize;
                b.iter(|| {
                    tag = tag.wrapping_add(1);
                    bus.dispatch(make_event(tag), DispatchMode::Serial);
                });
                drop(guards);
            },
        );
    }
    // Batch path used by `EventTx` for multi-input expansions (e.g. Failed).
    group.bench_function("serial_batch_2", |b| {
        let bus = EventBus::new();
        let sink = Arc::new(Sink(AtomicUsize::new(0)));
        let guard = bus.subscribe(TopicFilter::all(), sink.clone());
        let mut tag = 0usize;
        b.iter(|| {
            tag = tag.wrapping_add(1);
            bus.dispatch_serial_batch([make_event(tag), make_event(tag)]);
        });
        drop(guard);
    });
    group.finish();
}

/// Two typed nodes doing real-ish work: clone the payload string and append
/// to a scratch vector (mutation = Waterfall chain semantics).
#[derive(Default)]
struct Scratch(Vec<u8>);

struct MutatingNode;

#[async_trait::async_trait]
impl shannon_core::bus::GuardNode<Scratch> for MutatingNode {
    async fn guard(&self, ctx: &mut Scratch) -> Flow {
        ctx.0.push(b'.');
        if ctx.0.len() > 64 {
            ctx.0.clear();
        }
        Flow::Continue
    }
}

fn bench_waterfall(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("event_bus/waterfall_2_nodes", move |b| {
        b.iter_batched(
            || {
                let bus = EventBus::new();
                let pipe: shannon_core::bus::PipelineHandle<Scratch> =
                    bus.guard_pipeline("bench-waterfall");
                let _g1 = pipe.add_node("n1", Arc::new(MutatingNode));
                let _g2 = pipe.add_node("n2", Arc::new(MutatingNode));
                (bus, pipe)
            },
            |(bus, pipe)| {
                // criterion's async harness is behind a feature; driving the
                // (already cheap) chain through a block_on keeps the setup
                // outside the timed region identically.
                rt.block_on(async move {
                    let mut ctx = Scratch::default();
                    pipe.run(&mut ctx).await;
                    drop(bus);
                });
            },
            BatchSize::SmallInput,
        );
    });
}

/// Filtered dispatch: half the subscriptions match, half do not (routing
/// via kind+subtopic is the hub hot path).
fn bench_filtered(c: &mut Criterion) {
    c.bench_function("event_bus/emit_16_mixed_filters", |b| {
        let bus = EventBus::new();
        let sink = Arc::new(Sink(AtomicUsize::new(0)));
        let custom = shannon_core::bus::hook_trigger_event("Bench", serde_json::json!({}));
        assert_eq!(custom.kind, SessionEventKind::Custom);
        let _guards: Vec<_> = (0..8)
            .map(|_| bus.subscribe(TopicFilter::kind(SessionEventKind::Custom), sink.clone()))
            .chain((0..8).map(|_| {
                bus.subscribe(
                    TopicFilter::kinds([SessionEventKind::TodoWrite, SessionEventKind::ToolCall]),
                    sink.clone(),
                )
            }))
            .collect();
        let input = BusInput::Event(custom);
        b.iter(|| bus.dispatch(input.clone(), DispatchMode::Emit));
    });
}

criterion_group!(
    benches,
    bench_emit,
    bench_serial,
    bench_waterfall,
    bench_filtered
);
criterion_main!(benches);
