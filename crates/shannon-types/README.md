# shannon-types

Shared types for the Shannon project.

## Event Payloads

This crate defines wire-format event payloads shared between the Shannon engine and shells (Tauri-based, CLI, or future surfaces). All event types live in the [`events`](src/events.rs) module.

### JSON Schema

The crate generates a [JSON Schema](schema/events.schema.json) for all event payload types at build time using `schemars`. This schema:

- Documents the exact wire format for all 24 event types (23 payloads + `EventEnvelope`)
- Enables machine-readable validation and code generation in consumer languages
- Is committed to the repository so consumers can use it without building the crate

### When Schema Regenerates

The schema is automatically regenerated whenever `src/events.rs` changes (tracked via `cargo:rerun-if-changed`). After modifying event types:

1. Run `cargo build -p shannon-types` to regenerate the schema
2. Commit the updated `schema/events.schema.json` file
3. The CI check [`scripts/check-schema.sh`](../../scripts/check-schema.sh) validates consistency

### Schema Version Policy

All events currently use **schema version 1** (see [`EVENT_SCHEMA_VERSION`](src/events.rs#L26)). 

**When to bump the version:**
- Renaming a payload field
- Removing a payload field
- Changing a payload field's type
- Adding a new required field

**Version bump NOT required for:**
- Adding a new optional field (consumers ignore unknown fields via serde defaults)
- Adding a new event type (consumers process unknown event names gracefully)
- Documentation changes

When bumping, update [`EVENT_SCHEMA_VERSION`](src/events.rs#L26) and document the migration in `docs/architecture/d4-state-sync-protocol.md`.

### Consumers Using the Schema

**TypeScript example (quicktype):**
```bash
npm install -g quicktype
quicktype crates/shannon-types/schema/events.schema.json -o src/events.ts
```

**Python example (dataschema):**
```python
from jsonschema import validate
from shannon_types import QueryTextPayload

# Load schema
with open('crates/shannon-types/schema/events.schema.json') as f:
    schema = json.load(f)['QueryTextPayload']

# Validate instance
instance = QueryTextPayload(query_id="q1", content="hello")
validate(instance=instance, schema=schema)
```

## Other Types

The crate also provides common types used across the Shannon project:

- `EntityId` - UUID-based unique identifier
- `Timestamp` - UTC datetime with serde support
- `ShannonResult<T>` - Result alias with `ShannonError`
- `ShannonError` - Common error variants

See [`src/lib.rs`](src/lib.rs) for details.
