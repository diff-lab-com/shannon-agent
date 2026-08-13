/**
 * QR v2 payload generation for relay-mode pairing. The QR encodes a JSON object
 * the phone scans to discover both the LAN endpoint (for direct fallback) and
 * the relay endpoint (for remote access through shannon-relay).
 */

import { randomUUID } from "node:crypto";

export interface QrV2Options {
  /** WebSocket scheme for the LAN endpoint ("ws" or "wss"). */
  scheme: string;
  /** LAN IP for direct connection fallback. */
  host: string;
  /** Mobile server port for direct connection. */
  port: number;
  /** One-time pair token (also serves as E2E key material). */
  pairToken: string;
  /** Token expiry as epoch milliseconds. */
  expiresAt: number;
  /** Relay WebSocket URL (e.g. "wss://relay.shannon.example"). */
  relayUrl: string;
  /** Session ID for the relay (the host registers with this). */
  relaySessionId: string;
  /** Retained for forward compatibility; unused in key derivation. */
  hostE2EPubKey?: string | null;
}

/**
 * Generate the QR v2 JSON payload. Version 2 adds relay-mode fields so the phone
 * can choose between LAN-direct and relay-routed connection.
 */
export function generateQrV2Payload(opts: QrV2Options): Record<string, unknown> {
  return {
    v: 2,
    scheme: opts.scheme,
    host: opts.host,
    port: opts.port,
    token: opts.pairToken,
    exp: opts.expiresAt,
    mode: "relay",
    relayEndpoint: opts.relayUrl,
    relaySessionId: opts.relaySessionId,
    ...(opts.hostE2EPubKey ? { hostE2EPubKey: opts.hostE2EPubKey } : {}),
  };
}

/**
 * Generate a unique relay session ID (UUID without dashes).
 */
export function generateRelaySessionId(): string {
  return randomUUID().replace(/-/g, "");
}
