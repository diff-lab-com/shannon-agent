/**
 * Reusable NDJSON JSON-RPC dispatch logic, extracted from MobileServer so both
 * the direct-WS MobileServer and the relay host transport can share the same
 * method dispatch without duplicating the routing/error/notification pipeline.
 *
 * The caller provides a `send` callback, so the transport (raw WebSocket text
 * vs. E2E-encrypted binary through the relay) is fully abstracted.
 */

import { WebSocket } from "ws";

import type { Logger } from "../adapters/types.js";
import {
  JSONRPC_VERSION,
  ShannonError,
  parseNdjson,
  serializeFrame,
  type JsonRpcRequest,
  type JsonRpcResponse,
  type ShannonEvent,
  type ShannonEventNotification,
} from "./protocol.js";
import type { MethodContext, MethodHandlers } from "./server.js";

/**
 * Dispatch a single NDJSON text frame: split into records, parse, and route each
 * through `dispatchMessage`. Parse errors are sent back immediately.
 */
export async function dispatchNdjson(
  text: string,
  ctx: MethodContext,
  handlers: MethodHandlers,
  send: (data: string) => void,
  logger: Logger,
): Promise<void> {
  for (const record of parseNdjson(text)) {
    if (record === null) {
      send(serializeFrame(errorResponse(null, ShannonError.PARSE_ERROR, "malformed JSON")));
      continue;
    }
    await dispatchMessage(record, ctx, handlers, send, logger);
  }
}

/**
 * Dispatch a single parsed JSON-RPC record: validate, route to the handler,
 * stream events, and send the response. All output goes through `send`.
 */
export async function dispatchMessage(
  raw: unknown,
  ctx: MethodContext,
  handlers: MethodHandlers,
  send: (data: string) => void,
  _logger: Logger,
): Promise<void> {
  if (!isRequest(raw)) {
    send(
      serializeFrame(
        errorResponse(null, ShannonError.INVALID_REQUEST, "not a valid JSON-RPC request"),
      ),
    );
    return;
  }
  const handler = handlers[raw.method];
  if (!handler) {
    send(
      serializeFrame(
        errorResponse(raw.id, ShannonError.METHOD_NOT_FOUND, `no handler for ${raw.method}`),
      ),
    );
    return;
  }
  try {
    const outcome = await handler(raw.params, ctx);
    if (outcome.kind === "error") {
      send(serializeFrame(errorResponse(raw.id, outcome.code, outcome.message, outcome.data)));
      return;
    }
    if (outcome.kind === "stream") {
      for await (const ev of outcome.stream) {
        if (ctx.socket.readyState !== WebSocket.OPEN) return;
        send(serializeFrame(notification(ev)));
      }
    }
    send(serializeFrame(successResponse(raw.id, outcome.result)));
  } catch (err) {
    const message = (err as Error).message ?? "handler error";
    send(serializeFrame(errorResponse(raw.id, ShannonError.ENGINE_ERROR, message)));
  }
}

// ── helpers ──────────────────────────────────────────────────────────────────

function isRequest(v: unknown): v is JsonRpcRequest {
  if (typeof v !== "object" || v === null) return false;
  const r = v as Record<string, unknown>;
  return (
    r.jsonrpc === JSONRPC_VERSION &&
    typeof r.method === "string" &&
    (typeof r.id === "string" || typeof r.id === "number")
  );
}

function successResponse(id: string | number, result: unknown): JsonRpcResponse<unknown> {
  return { jsonrpc: JSONRPC_VERSION, id, result };
}

function errorResponse(
  id: string | number | null,
  code: number,
  message: string,
  data?: unknown,
): JsonRpcResponse<unknown> {
  const error: { code: number; message: string; data?: unknown } = { code, message };
  if (data !== undefined) error.data = data;
  return { jsonrpc: JSONRPC_VERSION, id, error };
}

function notification(event: ShannonEvent): ShannonEventNotification {
  return { jsonrpc: JSONRPC_VERSION, method: "shannon/event", params: event };
}
