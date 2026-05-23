//! Performance benchmarks for shannon-mcp protocol types
//!
//! Benchmarks cover:
//! - JSON-RPC message serialization/deserialization
//! - Large batch (100, 1000) of concurrent request/response pairs

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use shannon_mcp::protocol::{
    LoggingCapability, PromptsCapability, ResourcesCapability, ToolsCapability,
};
use shannon_mcp::{
    ClientCapabilities, ClientInfo, InitializeParams, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, ServerCapabilities, Tool, ToolAnnotations,
};

// ============================================================================
// JSON-RPC serialization/deserialization benchmarks
// ============================================================================

fn bench_jsonrpc_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("jsonrpc_serialize");

    // Request serialization
    let request = JsonRpcRequest::new("tools/list", Some(serde_json::json!({"cursor": null})));

    group.bench_function("request_serialize", |b| {
        b.iter(|| {
            let _ = serde_json::to_string(black_box(&request));
        })
    });

    // Response serialization
    let response = JsonRpcResponse::ok(
        "test-id-123",
        serde_json::json!({
            "tools": [
                {
                    "name": "Read",
                    "description": "Read file contents",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string" },
                            "offset": { "type": "integer" },
                            "limit": { "type": "integer" }
                        },
                        "required": ["file_path"]
                    }
                }
            ]
        }),
    );

    group.bench_function("response_serialize", |b| {
        b.iter(|| {
            let _ = serde_json::to_string(black_box(&response));
        })
    });

    // Notification serialization
    let notification = JsonRpcNotification::new("notifications/tools/list_changed", None);

    group.bench_function("notification_serialize", |b| {
        b.iter(|| {
            let _ = serde_json::to_string(black_box(&notification));
        })
    });

    group.finish();
}

fn bench_jsonrpc_deserialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("jsonrpc_deserialize");

    let request_json =
        r#"{"jsonrpc":"2.0","id":"abc-123","method":"tools/list","params":{"cursor":null}}"#;
    let response_json = r#"{"jsonrpc":"2.0","id":"abc-123","result":{"tools":[{"name":"Read","description":"Read file"}]}}"#;
    let notification_json = r#"{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":"hello"}}"#;

    group.bench_function("request_deserialize", |b| {
        b.iter(|| {
            let _: JsonRpcRequest = serde_json::from_str(black_box(request_json)).unwrap();
        })
    });

    group.bench_function("response_deserialize", |b| {
        b.iter(|| {
            let _: JsonRpcResponse = serde_json::from_str(black_box(response_json)).unwrap();
        })
    });

    group.bench_function("notification_deserialize", |b| {
        b.iter(|| {
            let _: JsonRpcNotification =
                serde_json::from_str(black_box(notification_json)).unwrap();
        })
    });

    group.finish();
}

// ============================================================================
// Batch request/response pair benchmarks
// ============================================================================

fn bench_batch_request_response(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_request_response");

    for batch_size in [100usize, 1000] {
        let requests: Vec<JsonRpcRequest> = (0..batch_size)
            .map(|i| {
                JsonRpcRequest::with_id(
                    format!("batch-{i}"),
                    "tools/call",
                    Some(serde_json::json!({
                        "name": format!("Tool{i}"),
                        "arguments": { "param": i }
                    })),
                )
            })
            .collect();

        let serialized_requests: Vec<String> = requests
            .iter()
            .map(|r| serde_json::to_string(r).unwrap())
            .collect();

        let responses: Vec<JsonRpcResponse> = (0..batch_size)
            .map(|i| {
                JsonRpcResponse::ok(
                    format!("batch-{i}"),
                    serde_json::json!({ "content": [{ "type": "text", "text": format!("result {i}") }] }),
                )
            })
            .collect();

        let serialized_responses: Vec<String> = responses
            .iter()
            .map(|r| serde_json::to_string(r).unwrap())
            .collect();

        group.bench_with_input(
            BenchmarkId::new("serialize_requests", batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| {
                    for req in &requests {
                        let _ = serde_json::to_string(black_box(req));
                    }
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("deserialize_requests", batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| {
                    for json in &serialized_requests {
                        let _: JsonRpcRequest = serde_json::from_str(black_box(json)).unwrap();
                    }
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("serialize_responses", batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| {
                    for res in &responses {
                        let _ = serde_json::to_string(black_box(res));
                    }
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("deserialize_responses", batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| {
                    for json in &serialized_responses {
                        let _: JsonRpcResponse = serde_json::from_str(black_box(json)).unwrap();
                    }
                })
            },
        );
    }

    group.finish();
}

// ============================================================================
// Complex type serialization round-trip
// ============================================================================

fn bench_complex_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("complex_types");

    let init_params = InitializeParams {
        protocol_version: "2024-11-05".to_string(),
        capabilities: ClientCapabilities {
            experimental: Some(serde_json::json!({ "feature": true })),
            sampling: None,
            resources: None,
            roots: None,
        },
        client_info: Some(ClientInfo {
            name: "shannon-bench".to_string(),
            version: "1.0.0".to_string(),
        }),
    };

    group.bench_function("initialize_params_roundtrip", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&init_params)).unwrap();
            let _: InitializeParams = serde_json::from_str(black_box(&json)).unwrap();
        })
    });

    let server_caps = ServerCapabilities {
        tools: Some(ToolsCapability { list_changed: true }),
        resources: Some(ResourcesCapability {
            subscribe: true,
            list_changed: true,
        }),
        prompts: Some(PromptsCapability {
            list_changed: false,
        }),
        logging: Some(LoggingCapability {
            level: "info".to_string(),
        }),
        completions: None,
    };

    group.bench_function("server_capabilities_roundtrip", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&server_caps)).unwrap();
            let _: ServerCapabilities = serde_json::from_str(black_box(&json)).unwrap();
        })
    });

    let tool = Tool {
        name: "Read".to_string(),
        description: "Read file contents from disk".to_string(),
        input_schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string", "description": "Path to the file" },
                "offset": { "type": "integer", "description": "Line offset" },
                "limit": { "type": "integer", "description": "Max lines to read" }
            },
            "required": ["file_path"]
        })),
        annotations: Some(ToolAnnotations {
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: true,
            open_world_hint: false,
        }),
    };

    group.bench_function("tool_with_annotations_roundtrip", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&tool)).unwrap();
            let _: Tool = serde_json::from_str(black_box(&json)).unwrap();
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_jsonrpc_serialize,
    bench_jsonrpc_deserialize,
    bench_batch_request_response,
    bench_complex_types,
);
criterion_main!(benches);
