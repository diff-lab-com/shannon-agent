/**
 * Bounded webhook body reading (review §P2-23).
 *
 * The IM webhook adapters verified platform signatures over the **raw**
 * body, but read that body without any size cap — a client (or a misbehaving
 * platform) could stream unbounded bytes into gateway memory before the
 * signature check ever ran. Every webhook entry point now reads through
 * [`readWebhookBody`], which enforces a 1 MiB cap: callers answer **413**
 * and stop reading. The cap sits far above any legitimate platform payload
 * (interactive-message callbacks are a few KiB).
 */

import { type IncomingMessage, type ServerResponse } from "node:http";

/** Maximum raw webhook body bytes accepted before signature verification. */
export const MAX_WEBHOOK_BODY_BYTES = 1024 * 1024;

/** Error thrown when a webhook body exceeds [`MAX_WEBHOOK_BODY_BYTES`]. */
export class WebhookBodyTooLargeError extends Error {
  readonly maxBytes: number;

  constructor(maxBytes: number) {
    super(`webhook body exceeds the ${maxBytes} byte limit`);
    this.name = "WebhookBodyTooLargeError";
    this.maxBytes = maxBytes;
  }
}

/**
 * Read a request body as UTF-8, enforcing `maxBytes` total.
 *
 * Rejects with [`WebhookBodyTooLargeError`] as soon as the cap is exceeded —
 * both up-front (honoring a lying-in-our-favor `Content-Length`) and mid-
 * stream (chunked / no-length bodies). The stream is paused, not destroyed,
 * so the caller can still write a 413 response.
 */
export function readWebhookBody(
  req: IncomingMessage,
  maxBytes: number = MAX_WEBHOOK_BODY_BYTES,
): Promise<string> {
  return new Promise((resolve, reject) => {
    const contentLength = Number(req.headers["content-length"] ?? "");
    if (Number.isFinite(contentLength) && contentLength > maxBytes) {
      req.pause();
      reject(new WebhookBodyTooLargeError(maxBytes));
      return;
    }

    const chunks: Buffer[] = [];
    let received = 0;
    let settled = false;

    req.on("data", (chunk: Buffer) => {
      if (settled) return;
      received += chunk.length;
      if (received > maxBytes) {
        settled = true;
        req.pause();
        reject(new WebhookBodyTooLargeError(maxBytes));
        return;
      }
      chunks.push(chunk);
    });
    req.on("end", () => {
      if (settled) return;
      settled = true;
      resolve(Buffer.concat(chunks).toString("utf8"));
    });
    req.on("error", (err) => {
      if (settled) return;
      settled = true;
      reject(err);
    });
  });
}

/**
 * Read a webhook body through the shared cap, answering **413** on the
 * response and returning `null` when the body is over-limit. Other errors
 * propagate to the caller's existing 500 path.
 */
export async function readWebhookBodyOr413(
  req: IncomingMessage,
  res: ServerResponse,
  maxBytes: number = MAX_WEBHOOK_BODY_BYTES,
): Promise<string | null> {
  try {
    return await readWebhookBody(req, maxBytes);
  } catch (err) {
    if (err instanceof WebhookBodyTooLargeError) {
      res.statusCode = 413;
      res.end("payload too large");
      return null;
    }
    throw err;
  }
}
