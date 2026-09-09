import { describe, expect, it } from "vitest";

import { type NormalizedInbound } from "../../types.js";
import {
  extractTelegramMedia,
  normalizeTelegramUpdate,
} from "../telegramAdapter.js";

function photoUpdate(text?: string) {
  return {
    message: {
      message_id: 7,
      chat: { id: 42, type: "private" },
      from: { id: 9, first_name: "Ada" },
      ...(text ? { text } : {}),
      photo: [
        { file_id: "small", width: 90, height: 90 },
        { file_id: "big", width: 1280, height: 720 },
        { file_id: "mid", width: 320, height: 320 },
      ],
    },
  };
}

describe("extractTelegramMedia", () => {
  it("picks the largest photo variant as jpeg", () => {
    const refs = extractTelegramMedia(photoUpdate().message);
    expect(refs).toEqual([{ fileId: "big", mimeType: "image/jpeg", name: null }]);
  });

  it("surfaces image documents with their filename", () => {
    const msg = {
      document: { file_id: "doc1", file_name: "art.png", mime_type: "image/png" },
    };
    expect(extractTelegramMedia(msg)).toEqual([
      { fileId: "doc1", mimeType: "image/png", name: "art.png" },
    ]);
  });

  it("ignores non-image documents and photo-less messages", () => {
    expect(extractTelegramMedia({ document: { file_id: "d", mime_type: "application/pdf" } })).toEqual([]);
    expect(extractTelegramMedia({ text: "hi" })).toEqual([]);
    expect(extractTelegramMedia(null)).toEqual([]);
  });
});

describe("normalizeTelegramUpdate with media (B4)", () => {
  it("normalizes caption-only photo messages (empty text)", () => {
    const update = {
      message: {
        message_id: 8,
        chat: { id: 42, type: "private" },
        from: { id: 9, first_name: "Ada" },
        caption: "what is this?",
        photo: [{ file_id: "p1", width: 100, height: 100 }],
      },
    };
    const n: NormalizedInbound | null = normalizeTelegramUpdate(update);
    expect(n).not.toBeNull();
    expect(n?.text).toBe("what is this?");
  });

  it("still rejects messages with nothing actionable", () => {
    const update = {
      message: {
        message_id: 9,
        chat: { id: 42, type: "private" },
        from: { id: 9 },
      },
    };
    expect(normalizeTelegramUpdate(update)).toBeNull();
  });
});
