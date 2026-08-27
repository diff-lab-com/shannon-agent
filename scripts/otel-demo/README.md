# otel-demo (§4.14)

One-command Jaeger + Grafana stack for verifying Shannon's OTLP bridge:

```bash
docker compose -f scripts/otel-demo/docker-compose.yml up -d
SHANNON_TELEMETRY=1 <run some sessions>   # spans land on localhost:4317
open http://localhost:16686               # service: shannon-code
```

Teardown: `docker compose -f scripts/otel-demo/docker-compose.yml down`.
Details, switch matrix, and span-folding rules live in
`crates/shannon-core/src/telemetry.rs` module docs.
