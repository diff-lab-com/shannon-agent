/**
 * Engine wire-type re-exports + runtime helpers.
 *
 * The contract itself (`types.gen.ts`) is generated from `shannon-api-protocol`
 * (Rust); this file is the manual layer that adds the gateway's runtime
 * ergonomic helpers (terminal-event detection, the `EngineEvent` alias,
 * etc.) on top of the generated shapes. Anything that mutates the wire
 * contract must mutate there first, then regenerate.
 *
 * Field names stay snake_case to match the wire 1:1 — no transform layer.
 */

import type {
  WsServerMessage,
  QueryRequest as GenQueryRequest,
} from "./types.gen.js";

/**
 * The engine is the only sender of these frames. `WsServerMessage` is the
 * full Rust-derived union; `EngineEvent` is the public-facing alias the
 * gateway runtime exposes. The two names line up because every `WsServerMessage`
 * variant is also an `EngineEvent` — there is no Rust variant that gets
 * filtered out.
 */
export type EngineEvent = WsServerMessage;
export type EngineEventType = WsServerMessage["type"];

/**
 * The Query request shape the gateway sends. Re-exported under the gateway's
 * existing name so call sites in `wsClient.ts` and adapters keep compiling.
 */
export type QueryRequest = GenQueryRequest;

/**
 * Variants that end a turn. The engine sends exactly one per query.
 */
export type TerminalEngineEvent = Extract<
  EngineEvent,
  { type: "completed" | "failed" | "cancelled" | "error" }
>;

export const TERMINAL_EVENT_TYPES: ReadonlySet<EngineEventType> = new Set([
  "completed",
  "failed",
  "cancelled",
  "error",
]);

export function isTerminalEvent(e: EngineEvent): e is TerminalEngineEvent {
  return TERMINAL_EVENT_TYPES.has(e.type);
}