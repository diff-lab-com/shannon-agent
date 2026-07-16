/**
 * Runtime helpers layered on the Rust-generated engine wire contract.
 *
 * Protocol declarations live exclusively in `types.gen.ts`. This module only
 * supplies aliases, narrowing helpers, and runtime constants used by gateway
 * code; field names remain identical to the generated wire shapes.
 */

import type { WsServerMessage } from "./types.gen.js";

export type EngineEvent = WsServerMessage;
export type EngineEventType = WsServerMessage["type"];

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

export function isTerminalEvent(
  event: EngineEvent,
): event is TerminalEngineEvent {
  return TERMINAL_EVENT_TYPES.has(event.type);
}
