/**
 * Inbound media → engine attachments pipeline (B4).
 *
 * The gateway's adapters normalize platform media into
 * `NormalizedInbound.media` (`MediaAttachment`, camelCase). The engine's
 * query paths accept `MessageAttachment` (snake_case, base64) and validate
 * MIME/size server-side — see `shannon_core::api_server::attachments_to_blocks`.
 *
 * This module is the single mapping point so every adapter faces the same
 * rules:
 *   - only `kind: "image"` with a whitelisted MIME type reaches the engine
 *     (png / jpeg / gif / webp — the multimodal adapters' set),
 *   - at most 8 attachments per turn,
 *   - at most 10 MiB per attachment after decode,
 *   - anything else is skipped with a warn log — never fails the turn.
 *
 * The same caps are enforced again by the engine; matching them here keeps
 * the user-facing behavior identical across transports.
 */

import { type Logger } from "../adapters/types.js";
import type { MediaAttachment } from "../adapters/types.js";
import type { MessageAttachment } from "../engine/types.gen.js";

/** MIME types the engine's multimodal adapters serialize. */
export const ENGINE_IMAGE_MIME_TYPES: readonly string[] = [
  "image/png",
  "image/jpeg",
  "image/gif",
  "image/webp",
];

/** Max attachments per turn — mirrors the engine's MAX_ATTACHMENTS. */
export const MAX_MEDIA_ATTACHMENTS = 8;

/** Max bytes per attachment after decode — mirrors the engine's limit. */
export const MAX_MEDIA_BYTES = 10 * 1024 * 1024;

export interface MediaPipelineOptions {
  /** Injectable for tests; defaults to the global fetch. */
  fetchImpl?: typeof fetch;
  logger?: Logger;
}

function noOpLogger(): Logger {
  return { debug: () => {}, info: () => {}, warn: () => {}, error: () => {} };
}

function engineMime(m: MediaAttachment): string | null {
  if (m.kind !== "image") return null;
  const mime = m.mimeType
    .toLowerCase()
    .split(";")[0]
    ?.trim();
  return mime && ENGINE_IMAGE_MIME_TYPES.includes(mime) ? mime : null;
}

/**
 * Map gateway inbound media → engine wire attachments. Downloads `url`
 * media via (injectable) fetch; `data` media is used inline. Skips — with
 * a warn — anything the engine can't ingest. Never throws: a broken
 * attachment degrades to a text-only turn, matching the engine's own
 * per-attachment rejection semantics.
 */
export async function toEngineAttachments(
  media: MediaAttachment[] | undefined,
  opts: MediaPipelineOptions = {},
): Promise<MessageAttachment[]> {
  if (!media || media.length === 0) return [];
  const fetchImpl = opts.fetchImpl ?? fetch;
  const logger = opts.logger ?? noOpLogger();

  if (media.length > MAX_MEDIA_ATTACHMENTS) {
    logger.warn(
      `media pipeline: ${media.length} attachments on one message, keeping the first ${MAX_MEDIA_ATTACHMENTS}`,
    );
  }

  const out: MessageAttachment[] = [];
  for (const m of media.slice(0, MAX_MEDIA_ATTACHMENTS)) {
    const mime = engineMime(m);
    if (!mime) {
      logger.warn(
        `media pipeline: skipping "${m.caption ?? "unnamed"}" (kind=${m.kind}, mime=${m.mimeType}: only ${ENGINE_IMAGE_MIME_TYPES.join(", ")} reach the engine)`,
      );
      continue;
    }
    try {
      const bytes = await mediaBytes(m, fetchImpl);
      if (bytes === null) {
        logger.warn(`media pipeline: skipping "${m.caption ?? "unnamed"}": no inline data and no fetchable url`);
        continue;
      }
      if (bytes.byteLength > MAX_MEDIA_BYTES) {
        logger.warn(
          `media pipeline: skipping "${m.caption ?? "unnamed"}": ${bytes.byteLength} bytes exceeds the ${MAX_MEDIA_BYTES} byte limit`,
        );
        continue;
      }
      out.push({
        name: m.caption ?? null,
        media_type: mime,
        data: Buffer.from(bytes).toString("base64"),
      });
    } catch (err) {
      logger.warn(`media pipeline: skipping "${m.caption ?? "unnamed"}": download failed: ${(err as Error).message}`);
    }
  }
  return out;
}

/** Inline bytes or fetch the URL. Returns null when neither is usable. */
async function mediaBytes(
  m: MediaAttachment,
  fetchImpl: typeof fetch,
): Promise<Uint8Array | null> {
  if (m.data) return m.data;
  if (!m.url) return null;
  const res = await fetchImpl(m.url);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const buf = await res.arrayBuffer();
  return new Uint8Array(buf);
}
