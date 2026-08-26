//! §4.8 verification standard ① — dual-dispatch reconciliation.
//!
//! During the bus migration the engine switched from a direct bypass
//! (`EventTx::send` → `SessionTee::record_query_event`) to the unified bus
//! (`EventTx::send` → `EventBus` → built-in [`L0TeeSubscriber`]). The two
//! paths must be observationally identical: this test drives the same
//! deterministic QueryEvent stream through both and requires the resulting
//! `events.jsonl` files to differ in **nothing but the wall-clock `ts_ns`
//! stamp** each row gets at write time.

#![allow(clippy::unwrap_used)]

mod reconciliation {
    use shannon_core::QueryEvent;
    use shannon_core::bus::{BusInput, BusSubscriber, DispatchMode, EventBus, TopicFilter};
    use shannon_core::session_log::{
        L0TeeSubscriber, SessionLogReader, TeeHandle, query_event_to_bus_inputs,
    };
    use tempfile::TempDir;
    use uuid::Uuid;

    /// Deterministic mixed stream exercising every legacy branch: mapping,
    /// fold-only inputs, turn closure, failure closure, rate-limit errors
    /// (which must NOT close a turn), plus one oversized payload that both
    /// paths must truncate identically.
    fn synthetic_stream() -> Vec<QueryEvent> {
        let q = Uuid::new_v4();
        let mut events = Vec::new();

        // Turn 1: text, tool call/result, usage folding, completion.
        for i in 0..40 {
            events.push(QueryEvent::Text {
                query_id: q,
                content: format!("chunk {i} — streaming token payload"),
            });
        }
        events.push(QueryEvent::Thinking {
            query_id: q,
            content: "reasoning about the task".into(),
        });
        events.push(QueryEvent::ToolUseRequest {
            query_id: q,
            tool_use_id: "toolu_1".into(),
            tool_name: "Bash".into(),
            tool_input: serde_json::json!({"command": "ls -la /tmp"}),
        });
        events.push(QueryEvent::ToolUseResult {
            query_id: q,
            tool_use_id: "toolu_1".into(),
            tool_name: "Bash".into(),
            result: "total 0".into(),
            is_error: false,
        });
        events.push(QueryEvent::Usage {
            query_id: q,
            input_tokens: 1_000,
            output_tokens: 200,
            cost_usd: 0.5,
            cache_creation_tokens: 10,
            cache_read_tokens: 20,
        });
        events.push(QueryEvent::TurnCompleted {
            query_id: q,
            turn_number: 1,
            tokens_used: 210,
        });
        events.push(QueryEvent::RateLimit {
            query_id: q,
            requests_used: 3,
            requests_limit: 10,
        });
        events.push(QueryEvent::Completed { query_id: q });

        // Bulk turns so the stream passes a few thousand events total:
        // repeated small probe events also cover chunk-flush batching parity.
        let bulk: Vec<QueryEvent> = (0..900)
            .flat_map(|turn| {
                let mut step = vec![
                    QueryEvent::Text {
                        query_id: q,
                        content: format!("turn {turn} hello"),
                    },
                    QueryEvent::ToolUseRequest {
                        query_id: q,
                        tool_use_id: format!("toolu_{turn}"),
                        tool_name: "echo".into(),
                        tool_input: serde_json::json!({"label": turn}),
                    },
                    QueryEvent::ToolUseResult {
                        query_id: q,
                        tool_use_id: format!("toolu_{turn}"),
                        tool_name: "echo".into(),
                        result: format!("out-{turn}"),
                        is_error: false,
                    },
                    QueryEvent::Usage {
                        query_id: q,
                        input_tokens: 100 + turn as u64,
                        output_tokens: 10,
                        cost_usd: 0.01,
                        cache_creation_tokens: 0,
                        cache_read_tokens: 5,
                    },
                ];
                if turn % 7 == 0 {
                    step.push(QueryEvent::Warning {
                        query_id: q,
                        message: format!("recoverable hiccup {turn}"),
                    });
                }
                if turn % 11 == 0 {
                    step.push(QueryEvent::Cost {
                        query_id: q,
                        total_cost_usd: 1.25,
                        input_tokens: 50,
                        output_tokens: 60,
                    });
                }
                if turn % 13 == 0 {
                    step.push(QueryEvent::Progress {
                        query_id: q,
                        message: format!("step {turn}"),
                    });
                }
                if turn % 17 == 0 {
                    step.push(QueryEvent::Info {
                        query_id: q,
                        message: format!("compaction metrics {turn}"),
                    });
                }
                if turn % 19 == 0 {
                    step.push(QueryEvent::ConversationUpdate {
                        query_id: q,
                        messages: Vec::new(),
                    });
                }
                if turn % 23 == 0 {
                    step.push(QueryEvent::ToolProgress {
                        query_id: q,
                        tool_use_id: format!("toolu_{turn}"),
                        tool_name: "echo".into(),
                        progress: 0.5,
                        message: "halfway".into(),
                    });
                }
                step
            })
            .collect();
        assert!(
            bulk.len() >= 3_000,
            "reconciliation stream spans a few thousand events (plan §4.8: N 千事件零差异)"
        );
        events.extend(bulk);

        // Oversized payload: truncation must be byte-identical on both paths.
        events.push(QueryEvent::ToolUseResult {
            query_id: q,
            tool_use_id: "big".into(),
            tool_name: "dump".into(),
            result: "x".repeat(400 * 1024),
            is_error: false,
        });

        // Turn 2 ends in failure: error row + closed-turn pairing.
        events.push(QueryEvent::Failed {
            query_id: q,
            error: "boom: rate limited".into(),
        });
        events
    }

    struct CountingSubscriber(std::sync::atomic::AtomicUsize);

    impl CountingSubscriber {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self(std::sync::atomic::AtomicUsize::new(0)))
        }
    }

    impl BusSubscriber for CountingSubscriber {
        fn on_input(&self, _input: &BusInput) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn normalized_lines(path: &std::path::Path) -> Vec<String> {
        let text = std::fs::read_to_string(path).expect("events.jsonl");
        text.lines()
            .map(|line| {
                let mut value: serde_json::Value =
                    serde_json::from_str(line).expect("each line is valid JSON");
                if let Some(obj) = value.as_object_mut() {
                    obj.remove("ts_ns");
                }
                value.to_string()
            })
            .collect()
    }

    #[test]
    fn bus_path_and_direct_bypass_path_produce_identical_logs() {
        let dir_a = TempDir::new().unwrap();
        let dir_b = TempDir::new().unwrap();
        let events = synthetic_stream();
        let event_count = events.len();
        assert!(event_count >= 400, "reconciliation stream is substantive");

        // ---- Path A (legacy): direct bypass, exactly what EventTx did. ----
        {
            let mut tee = shannon_core::session_log::SessionTee::open_in_dir(
                dir_a.path(),
                "sess-recon",
                "claude-x",
                Some("anthropic"),
            );
            tee.record_user_message("user asks something");
            tee.record_turn_start(Some("q-recon".to_string()));
            for event in &events {
                tee.record_query_event(event);
            }
            drop(tee); // closes the open turn as interrupted
        }

        // ---- Path B (new): bus with the L0 writer as built-in subscriber.
        //
        // Ordering note: Path B must not rely on Drop-driven close — after
        // the Failed event every remaining event must still land, matching
        // Path A where writes continued after Failed too. Dropping the tee
        // closes any *still-open* turn afterwards; since Failed already
        // closed it, no extra interrupted turn/end may appear on either
        // side.
        let spy = CountingSubscriber::new();
        {
            let tee =
                TeeHandle::open_in_dir(dir_b.path(), "sess-recon", "claude-x", Some("anthropic"));
            let l0_tee = tee.clone();
            let bus = EventBus::new();
            let guard_l0 = bus.subscribe(
                TopicFilter::all(),
                std::sync::Arc::new(L0TeeSubscriber::new(l0_tee)),
            );
            let guard_spy = bus.subscribe(TopicFilter::all(), spy.clone());

            tee.record_user_message("user asks something");
            tee.record_turn_start(Some("q-recon".to_string()));
            for event in &events {
                let inputs = query_event_to_bus_inputs(event);
                bus.dispatch_serial_batch(inputs);
            }
            drop(guard_l0);
            drop(guard_spy);
            drop(bus);
            drop(tee);
        }

        // Every produced bus input was delivered to subscribers...
        let delivered = spy.0.load(std::sync::atomic::Ordering::SeqCst);
        let expected_inputs: usize = events
            .iter()
            .map(|e| query_event_to_bus_inputs(e).len())
            .sum();
        assert_eq!(delivered, expected_inputs, "no dropped inputs");
        assert!(expected_inputs > 0);

        // Zero-difference assertion modulo ts_ns.
        let left = normalized_lines(&shannon_core::session_log::session_events_path(
            dir_a.path(),
            "sess-recon",
        ));
        let right = normalized_lines(&shannon_core::session_log::session_events_path(
            dir_b.path(),
            "sess-recon",
        ));
        assert_eq!(left.len(), right.len(), "same number of durable rows");
        for (idx, (a, b)) in left.iter().zip(right.iter()).enumerate() {
            assert_eq!(a, b, "row {idx} diverges between bypass and bus paths");
        }
    }

    #[test]
    fn reader_rejects_nothing_new_bus_log_still_required_only() {
        // Both paths remain plain required-semantics logs: the reader parses
        // them without opting into unknown kinds.
        let dir = TempDir::new().unwrap();
        let events = synthetic_stream();
        {
            let mut tee = shannon_core::session_log::SessionTee::open_in_dir(
                dir.path(),
                "sess-read",
                "m",
                None,
            );
            for event in &events {
                tee.record_query_event(event);
            }
            drop(tee);
        }
        let reader = SessionLogReader::open(shannon_core::session_log::session_events_path(
            dir.path(),
            "sess-read",
        ))
        .unwrap();
        let count = reader.read_events(false).unwrap().len();
        assert!(
            count > 100,
            "large log parsed with strict semantics: {count}"
        );
    }
}
