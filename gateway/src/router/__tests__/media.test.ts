import { describe, expect, it } from "vitest";

import { type Logger, type MediaAttachment } from "../../adapters/types.js";
import type { MessageAttachment } from "../../engine/types.gen.js";
import {
  MAX_MEDIA_ATTACHMENTS,
  MAX_MEDIA_BYTES,
  toEngineAttachments,
} from "../media.js";

const noopLogger: Logger = { debug() {}, info() {}, warn() {}, error() {} };

function pngData(bytes: number): Uint8Array {
  return new Uint8Array(bytes).fill(0x89);
}

function image(over: Partial<MediaAttachment> = {}): MediaAttachment {
  return {
    kind: "image",
    mimeType: "image/png",
    data: pngData(4),
    caption: "shot.png",
    ...over,
  };
}

async function capture(
  media: MediaAttachment[] | undefined,
  opts: Parameters<typeof toEngineAttachments>[1] = {},
): Promise<{ out: MessageAttachment[]; warnings: string[] }> {
  const warnings: string[] = [];
  const logger: Logger = { ...noopLogger, warn: (m: string) => warnings.push(m) };
  const out = await toEngineAttachments(media, { logger, ...opts });
  return { out, warnings };
}

describe("toEngineAttachments", () => {
  it("maps inline image data to wire attachments", async () => {
    const { out } = await capture([image()]);
    expect(out).toHaveLength(1);
    expect(out[0]).toEqual({
      name: "shot.png",
      media_type: "image/png",
      // 4 × 0x89 base64
      data: Buffer.from(pngData(4)).toString("base64"),
    });
  });

  it("returns empty for absent/empty media", async () => {
    expect(await toEngineAttachments(undefined)).toEqual([]);
    expect(await toEngineAttachments([])).toEqual([]);
  });

  it("skips non-image kinds and unsupported MIME types", async () => {
    const { out, warnings } = await capture([
      image({ kind: "video", mimeType: "video/mp4" }),
      image({ mimeType: "application/pdf" }),
      image(),
    ]);
    expect(out).toHaveLength(1);
    expect(warnings).toHaveLength(2);
    expect(warnings).toEqual([expect.stringContaining("only image/png"), expect.stringContaining("only image/png")]);
  });

  it("downloads url media through the injected fetch", async () => {
    const bytes = pngData(6);
    const fetchImpl = (async () =>
      new Response(bytes, { status: 200 })) as unknown as typeof fetch;
    const { out } = await capture([image({ data: undefined, url: "https://cdn.example/x.png" })], {
      fetchImpl,
    });
    expect(out).toHaveLength(1);
    expect(out[0]?.data).toBe(Buffer.from(bytes).toString("base64"));
  });

  it("skips items with neither data nor url", async () => {
    const { out, warnings } = await capture([image({ data: undefined })]);
    expect(out).toHaveLength(0);
    expect(warnings).toEqual([expect.stringContaining("no inline data")]);
  });

  it("skips failed downloads without throwing", async () => {
    const fetchImpl = (async () => new Response(null, { status: 404 })) as unknown as typeof fetch;
    const { out } = await capture(
      [image({ data: undefined, url: "https://cdn.example/x.png" }), image()],
      { fetchImpl },
    );
    expect(out).toHaveLength(1);
  });

  it("skips oversized payloads", async () => {
    const { out, warnings } = await capture([image({ data: pngData(MAX_MEDIA_BYTES + 1) })]);
    expect(out).toHaveLength(0);
    expect(warnings).toEqual([expect.stringContaining("byte limit")]);
  });

  it("keeps at most MAX_MEDIA_ATTACHMENTS items", async () => {
    const many = Array.from({ length: MAX_MEDIA_ATTACHMENTS + 3 }, () => image());
    const { out, warnings } = await capture(many);
    expect(out).toHaveLength(MAX_MEDIA_ATTACHMENTS);
    expect(warnings.some((w) => w.includes("keeping the first"))).toBe(true);
  });
});
