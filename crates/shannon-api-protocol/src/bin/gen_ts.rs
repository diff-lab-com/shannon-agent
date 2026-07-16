//! `gen-ts` — codegen binary that emits `gateway/src/engine/types.gen.ts`
//! from the `shannon-api-protocol` types. The single source of truth for the
//! wire schema is the Rust crate; this binary renders the TypeScript view.
//!
//! ## How it works
//!
//! 1. For every published type in [`shannon_api_protocol`], derive a
//!    [`schemars::schema_for!`] JSON Schema.
//! 2. Walk the schema and emit a deterministic, alphabetically sorted TypeScript
//!    module. Field names, casing, optionality, and discriminated unions
//!    match the serde-derived Rust output byte-for-byte.
//! 3. Write the file to `gateway/src/engine/types.gen.ts`. The path is
//!    resolved relative to the workspace root by walking up from `CARGO_MANIFEST_DIR`,
//!    so `cargo run -p shannon-api-protocol --bin gen-ts` works from anywhere.
//!
//! The output is checked in (it is the file the gateway imports). Re-run the
//! binary whenever the Rust types change; the `gateway pnpm typecheck` gate
//! in `verify-migration.sh A` would fail if the file were stale, so the
//! contract is enforced end-to-end.

use schemars::JsonSchema;
use schemars::schema::{InstanceType, RootSchema, Schema, SchemaObject, SingleOrVec};
use shannon_api_protocol::{
    ApprovalDecision, ApprovalRespondRequest, HealthResponse, ModelInfo, ModelsResponse,
    PROTOCOL_VERSION, QueryRequest, QueryResponse, ToolEntry, ToolsListResponse, UsageInfo,
    WsClientMessage, WsServerMessage,
};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

#[derive(Debug)]
struct GenError(String);
impl std::fmt::Display for GenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for GenError {}
impl GenError {
    fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl From<std::fmt::Error> for GenError {
    fn from(e: std::fmt::Error) -> Self {
        GenError::new(format!("fmt error: {e}"))
    }
}

/// One entry: the TS name we want emitted and the schemars-rooted value we
/// use to derive it. We emit the types in this exact order to keep the
/// generated file diff-friendly.
struct TypeEntry {
    ts_name: &'static str,
    root: RootSchema,
    /// When true, emit `export interface ... { ... }`. When false, emit a
    /// discriminated `type ... = ... | ...` union (oneOf in schemars).
    is_struct: bool,
}

/// Shared render context — the schemars `definitions` table is used to
/// resolve `$ref` strings into named TS interfaces.
struct Ctx<'a> {
    #[allow(dead_code)]
    defs: &'a std::collections::BTreeMap<String, Schema>,
}

fn collect_entries() -> Vec<TypeEntry> {
    vec![
        entry_struct::<QueryRequest>("QueryRequest"),
        entry_struct::<QueryResponse>("QueryResponse"),
        entry_struct::<UsageInfo>("UsageInfo"),
        entry_struct::<HealthResponse>("HealthResponse"),
        entry_struct::<ModelInfo>("ModelInfo"),
        entry_struct::<ModelsResponse>("ModelsResponse"),
        entry_struct::<ToolEntry>("ToolEntry"),
        entry_struct::<ToolsListResponse>("ToolsListResponse"),
        entry_struct::<ApprovalRespondRequest>("ApprovalRespondRequest"),
        entry_enum_simple("ApprovalDecision"),
        entry_tagged_enum::<WsClientMessage>("WsClientMessage"),
        entry_tagged_enum::<WsServerMessage>("WsServerMessage"),
    ]
}

fn entry_struct<T: JsonSchema>(ts_name: &'static str) -> TypeEntry {
    TypeEntry {
        ts_name,
        root: schemars::schema_for!(T),
        is_struct: true,
    }
}

fn entry_tagged_enum<T: JsonSchema>(ts_name: &'static str) -> TypeEntry {
    TypeEntry {
        ts_name,
        root: schemars::schema_for!(T),
        is_struct: false,
    }
}

/// `ApprovalDecision` is a plain enum (no `tag`) — its schemars rendering is
/// a `oneOf` over single-string values, which we materialise as a union of
/// string literals to match the wire shape exactly.
fn entry_enum_simple(ts_name: &'static str) -> TypeEntry {
    TypeEntry {
        ts_name,
        root: schemars::schema_for!(ApprovalDecision),
        is_struct: false,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let entries = collect_entries();
    let mut out = String::new();

    // Header: stable, checked-in; explains what the file is and where it came from.
    writeln!(
        out,
        "/**\n * GENERATED FILE — DO NOT EDIT BY HAND.\n *\n * Source of truth: `shannon-api-protocol` (Rust).\n * Generator:     `cargo run -p shannon-api-protocol --bin gen-ts`.\n * Protocol:      v{PROTOCOL_VERSION}\n *\n * Field names, casing, and discriminated unions match the serde-derived\n * Rust types 1:1. Anything that mutates must mutate there first and be\n * regenerated here. The runtime lives in this same folder and consumes\n * these types directly; only the contract is generated.\n */\n"
    )?;

    // Collect every named struct definition we will reference, so we can
    // resolve `$ref`s uniformly and inline referenced structs on demand.
    for entry in &entries {
        // We thread only the entry's own `definitions`; everything we generate
        // for top-level types stays in one entry, no cross-type sharing.
        let ctx = Ctx {
            defs: &entry.root.definitions,
        };
        render_type(&mut out, entry, &ctx)?;
        out.push('\n');
    }

    // Stable protocol_version constant — emitted so consumers can branch on it
    // without parsing the WS greeting.
    writeln!(
        out,
        "export const PROTOCOL_VERSION = \"{PROTOCOL_VERSION}\" as const;"
    )?;

    let dest = resolve_output_path();
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, out)?;
    eprintln!("wrote {}", dest.display());
    Ok(())
}

fn resolve_output_path() -> PathBuf {
    // Manifest dir is `…/crates/shannon-api-protocol`; the workspace root is two
    // levels up. We refuse to write anywhere else so the binary is location
    // independent (no sibling checkouts required) but stays scoped.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root must have a parent two levels above the manifest dir");
    workspace_root.join("gateway/src/engine/types.gen.ts")
}

// ── Renderer ────────────────────────────────────────────────────────────

fn schema_obj(s: &Schema) -> Option<&SchemaObject> {
    match s {
        Schema::Object(o) => Some(o),
        Schema::Bool(_) => None,
    }
}

fn render_type(out: &mut String, entry: &TypeEntry, ctx: &Ctx<'_>) -> Result<(), GenError> {
    let root = &entry.root.schema;

    if entry.is_struct {
        return render_struct(out, entry.ts_name, root, ctx);
    }

    // Tagged enums (serde tag = "type") come back from schemars as a `oneOf`
    // where each variant carries a `type` property. Plain unit-only enums
    // (e.g. ApprovalDecision) are emitted as a single object with a string
    // `type` and an `enum` of string literals.
    if let Some(one_of) = one_of_of(root) {
        let first_obj = one_of
            .first()
            .and_then(schema_obj)
            .ok_or_else(|| GenError::new("oneOf variant is not an object"))?;

        let first_variant_has_type = first_obj
            .object
            .as_ref()
            .map(|o| o.properties.contains_key("type"))
            .unwrap_or(false);

        if first_variant_has_type {
            return render_tagged_enum(out, entry.ts_name, one_of, ctx);
        }
        return render_string_enum_from_one_of(out, entry.ts_name, one_of);
    }

    // Plain unit-only enum (single object, string `type`, `enum` array).
    if let Some(enum_values) = root.enum_values.as_ref() {
        if matches!(root.instance_type.as_ref(), Some(SingleOrVec::Single(boxed)) if matches!(**boxed, InstanceType::String))
        {
            return render_inline_string_enum(out, entry.ts_name, enum_values);
        }
    }

    Err(GenError::new(
        "enum schema has no oneOf and is not a string-enum; cannot render",
    ))
}

fn one_of_of(root: &SchemaObject) -> Option<&Vec<Schema>> {
    root.subschemas.as_ref()?.one_of.as_ref()
}

fn any_of_of(root: &SchemaObject) -> Option<&Vec<Schema>> {
    root.subschemas.as_ref()?.any_of.as_ref()
}

fn render_struct(
    out: &mut String,
    name: &str,
    root: &SchemaObject,
    ctx: &Ctx,
) -> Result<(), GenError> {
    let (props, required): (BTreeMap<String, Schema>, std::collections::BTreeSet<String>) =
        match root.object.as_ref() {
            Some(o) => (o.properties.clone(), o.required.iter().cloned().collect()),
            None => (BTreeMap::new(), std::collections::BTreeSet::new()),
        };
    writeln!(out, "export interface {name} {{")?;
    // Stable order: alphabetical by property name. Diff-friendly.
    let mut keys: Vec<&String> = props.keys().collect();
    keys.sort();
    for k in keys {
        let prop_schema = &props[k];
        let is_required = required.contains(k);
        // A `default` value (e.g. `#[serde(default)]` on the Rust side) means
        // the field is always emitted on the wire, so we keep it required
        // in TS — even when schemars does not list it under `required`. This
        // matches the existing hand-written `gateway/src/engine/types.ts`.
        let has_default = schema_obj(prop_schema)
            .and_then(|o| o.metadata.as_ref())
            .and_then(|m| m.default.as_ref())
            .is_some();
        let optional = !is_required && !has_default;
        let type_str = ts_type_for_schema(prop_schema, optional, ctx)?;
        writeln!(out, "  {k}{}: {type_str};", if optional { "?" } else { "" })?;
    }
    writeln!(out, "}}")?;
    Ok(())
}

fn render_tagged_enum(
    out: &mut String,
    name: &str,
    variants: &[Schema],
    ctx: &Ctx,
) -> Result<(), GenError> {
    let mut variant_names: Vec<String> = Vec::new();
    for variant in variants {
        let vobj =
            schema_obj(variant).ok_or_else(|| GenError::new("tagged variant is not an object"))?;
        let tag = extract_tag(vobj)?;
        let type_name = format!("{name}{}", to_pascal(&tag));
        writeln!(out, "export interface {type_name} {{")?;
        writeln!(out, "  type: \"{tag}\";")?;
        let (props, required) = match vobj.object.as_ref() {
            Some(o) => (
                o.properties.clone(),
                o.required
                    .iter()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>(),
            ),
            None => (BTreeMap::new(), std::collections::BTreeSet::new()),
        };
        let mut keys: Vec<&String> = props
            .iter()
            .filter(|(k, _)| *k != "type")
            .map(|(k, _)| k)
            .collect();
        keys.sort();
        for k in keys {
            let prop_schema = &props[k];
            let optional = !required.contains(k);
            let type_str = ts_type_for_schema(prop_schema, optional, ctx)?;
            writeln!(out, "  {k}{}: {type_str};", if optional { "?" } else { "" })?;
        }
        writeln!(out, "}}")?;
        variant_names.push(type_name);
    }

    let union = variant_names
        .iter()
        .map(|n| format!("  | {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    writeln!(out)?;
    writeln!(out, "export type {name} =\n{union};")?;
    Ok(())
}

/// Render a plain unit-only enum that arrives as a `oneOf` whose each variant
/// is just `{"type": "string", "enum": ["..."]}`.
fn render_string_enum_from_one_of(
    out: &mut String,
    name: &str,
    variants: &[Schema],
) -> Result<(), GenError> {
    let mut values: Vec<String> = Vec::new();
    for v in variants {
        let vobj =
            schema_obj(v).ok_or_else(|| GenError::new("string enum variant is not an object"))?;
        if let Some(enum_vals) = &vobj.enum_values {
            for ev in enum_vals {
                if let Some(s) = ev.as_str() {
                    values.push(format!("\"{s}\""));
                }
            }
        }
    }
    render_string_enum_values(out, name, &mut values);
    Ok(())
}

/// Render a plain unit-only enum that arrives as a single object with
/// `type: "string"` and `enum: [...]`.
fn render_inline_string_enum(
    out: &mut String,
    name: &str,
    enum_values: &[serde_json::Value],
) -> Result<(), GenError> {
    let mut values: Vec<String> = enum_values
        .iter()
        .filter_map(|v| v.as_str().map(|s| format!("\"{s}\"")))
        .collect();
    render_string_enum_values(out, name, &mut values);
    Ok(())
}

fn render_string_enum_values(out: &mut String, name: &str, values: &mut Vec<String>) {
    values.sort();
    values.dedup();
    let union = values
        .iter()
        .map(|s| format!("  | {s}"))
        .collect::<Vec<_>>()
        .join("\n");
    let _ = writeln!(out, "export type {name} =\n{union};");
}

fn extract_tag(vobj: &SchemaObject) -> Result<String, GenError> {
    let props = vobj
        .object
        .as_ref()
        .map(|o| o.properties.clone())
        .ok_or_else(|| GenError::new("variant has no properties"))?;
    let tag_schema = props
        .get("type")
        .ok_or_else(|| GenError::new("variant has no 'type' property"))?;
    let tag_obj =
        schema_obj(tag_schema).ok_or_else(|| GenError::new("variant 'type' is not an object"))?;
    if let Some(enum_vals) = &tag_obj.enum_values {
        if let Some(first) = enum_vals.first() {
            if let Some(s) = first.as_str() {
                return Ok(s.to_string());
            }
        }
    }
    Err(GenError::new("could not extract tag value"))
}

fn to_pascal(s: &str) -> String {
    s.split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn ts_type_for_schema(schema: &Schema, optional: bool, ctx: &Ctx<'_>) -> Result<String, GenError> {
    // Boolean schemas (true / false) are how schemars represents
    // "match anything" for a `serde_json::Value` field. Map to a permissive
    // TS type — the runtime value is opaque anyway.
    if let Schema::Bool(true) = schema {
        return Ok("unknown".to_string());
    }
    if let Schema::Bool(false) = schema {
        return Ok("never".to_string());
    }
    let obj = schema_obj(schema).ok_or_else(|| GenError::new("nested schema is not an object"))?;
    // Resolve `$ref` early so a referenced struct renders as its named
    // interface (so we can refer back to it elsewhere in the same file).
    if let Some(reference) = obj.reference.as_ref() {
        if let Some(name) = reference.strip_prefix("#/definitions/") {
            return Ok(name.to_string());
        }
    }
    let base = ts_type_for_object(obj, ctx)?;
    // For `Option<T>` schemars produces `anyOf: [T, null]`. The `ts_type_for_object`
    // pass already emits ` | null` when null is in the union, so we only need to
    // also do it here when (a) the caller says the field is optional AND (b) the
    // schema itself doesn't already accept null. Otherwise we duplicate `| null`.
    let is_already_nullable = base.contains(" | null");
    Ok(if optional && !is_already_nullable {
        format!("{base} | null")
    } else {
        base
    })
}

fn ts_type_for_object(obj: &SchemaObject, ctx: &Ctx<'_>) -> Result<String, GenError> {
    // `$ref` is hoisted out of `ts_type_for_schema`; this function should
    // never be called on a `$ref` schema directly. Treat as unknown so we
    // still produce parseable TS rather than panicking.
    if obj.reference.is_some() {
        return Ok("unknown".to_string());
    }
    // Array first: `Vec<T>` is `{"type": "array", "items": {...}}` in schemars.
    if let Some(SingleOrVec::Single(boxed)) = &obj.instance_type {
        if matches!(**boxed, InstanceType::Array) {
            if let Some(items) = obj.array.as_ref().and_then(|a| a.items.as_ref()) {
                let inner = match items {
                    SingleOrVec::Single(s) => ts_type_for_schema(s.as_ref(), false, ctx)?,
                    SingleOrVec::Vec(v) => {
                        let parts: Vec<String> = v
                            .iter()
                            .map(|s| ts_type_for_schema(s, false, ctx))
                            .collect::<Result<_, _>>()?;
                        parts.join(" | ")
                    }
                };
                return Ok(format!("{inner}[]"));
            }
            return Ok("unknown[]".to_string());
        }
    }
    if let Some(it) = &obj.instance_type {
        return Ok(primitive_type(it));
    }
    // anyOf with one null variant → nullable shorthand.
    if let Some(any_of) = any_of_of(obj) {
        let mut parts = Vec::new();
        let mut saw_null = false;
        for s in any_of {
            // A `$ref` variant must be resolved via `ts_type_for_schema` so
            // we hit the reference shortcut rather than dropping into "unknown".
            if let Some(inner) = schema_obj(s) {
                if inner.reference.is_some() {
                    parts.push(ts_type_for_schema(s, false, ctx)?);
                    continue;
                }
                if matches!(
                    &inner.instance_type,
                    Some(SingleOrVec::Single(boxed)) if matches!(**boxed, InstanceType::Null)
                ) {
                    saw_null = true;
                    continue;
                }
                parts.push(ts_type_for_object(inner, ctx)?);
            }
        }
        if parts.len() == 1 {
            return Ok(if saw_null {
                format!("{} | null", parts[0])
            } else {
                parts.remove(0)
            });
        }
        if !parts.is_empty() {
            let mut s = parts.join(" | ");
            if saw_null {
                s.push_str(" | null");
            }
            return Ok(s);
        }
    }
    // Plain object with named properties → render an inline structural type.
    if let Some(o) = obj.object.as_ref() {
        if !o.properties.is_empty() {
            return Ok(inline_struct(o, ctx));
        }
    }
    Ok("unknown".to_string())
}

/// Render an inline structural type for an `object` with named properties.
/// Used when a field references a struct (e.g. `Vec<ModelInfo>` expands to
/// `{ id: string; provider: string }[]`). Field ordering is alphabetical so
/// the output is diff-friendly.
fn inline_struct(o: &schemars::schema::ObjectValidation, ctx: &Ctx<'_>) -> String {
    let mut keys: Vec<&String> = o.properties.keys().collect();
    keys.sort();
    let parts: Vec<String> = keys
        .into_iter()
        .map(|k| {
            let prop_schema = &o.properties[k];
            let is_required = o.required.contains(k);
            let optional = !is_required;
            let ty = ts_type_for_schema(prop_schema, optional, ctx)
                .unwrap_or_else(|_| "unknown".to_string());
            format!("{}{}: {};", k, if optional { "?" } else { "" }, ty)
        })
        .collect();
    format!("{{ {} }}", parts.join(" "))
}

fn primitive_type(t: &SingleOrVec<InstanceType>) -> String {
    match t {
        SingleOrVec::Single(s) => primitive_type_single(s),
        SingleOrVec::Vec(ms) => ms
            .iter()
            .map(primitive_type_single)
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

fn primitive_type_single(t: &InstanceType) -> String {
    match t {
        InstanceType::String => "string".to_string(),
        InstanceType::Integer | InstanceType::Number => "number".to_string(),
        InstanceType::Boolean => "boolean".to_string(),
        InstanceType::Null => "null".to_string(),
        InstanceType::Array => "unknown[]".to_string(),
        InstanceType::Object => "Record<string, unknown>".to_string(),
    }
}
