import { describe, expect, it } from "vitest";

import {
  extractDiscordMedia,
  normalizeDiscordMessage,
} from "../discordAdapter.js";

const author = { id: "u1", username: "ada" };

describe("extractDiscordMedia", () => {
  it("surfaces image attachments as url media", () => {
    const msg = {
      id: "m1",
      channel_id: "c1",
      content: "",
      author,
      attachments: [
        {
          id: "a1",
          filename: "shot.png",
          content_type: "image/png",
          url: "https://cdn.discordapp.com/attachments/c1/a1/shot.png",
          size: 1234,
        },
        { id: "a2", filename: "notes.pdf", content_type: "application/pdf", url: "https://x/y.pdf" },
      ],
    };
    const media = extractDiscordMedia(msg);
    expect(media).toHaveLength(1);
    expect(media[0]).toMatchObject({
      kind: "image",
      mimeType: "image/png",
      url: "https://cdn.discordapp.com/attachments/c1/a1/shot.png",
      caption: "shot.png",
    });
  });

  it("returns empty without attachments", () => {
    expect(extractDiscordMedia({ id: "m", channel_id: "c", content: "hi", author })).toEqual([]);
    expect(extractDiscordMedia(null)).toEqual([]);
  });
});

describe("normalizeDiscordMessage with media (B4)", () => {
  it("accepts image-only messages (empty content)", () => {
    const n = normalizeDiscordMessage({
      id: "m2",
      channel_id: "c1",
      content: "",
      author,
      attachments: [
        { id: "a1", filename: "pic.jpg", content_type: "image/jpeg", url: "https://cdn/x.jpg" },
      ],
    });
    expect(n).not.toBeNull();
    expect(n?.text).toBe("");
    expect(n?.media).toHaveLength(1);
  });

  it("still rejects plain empty messages", () => {
    expect(
      normalizeDiscordMessage({ id: "m3", channel_id: "c1", content: "", author }),
    ).toBeNull();
  });
});
