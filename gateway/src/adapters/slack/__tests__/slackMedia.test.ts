import { describe, expect, it } from "vitest";

import { extractSlackMedia, normalizeSlackEvent } from "../slackAdapter.js";

const fileEvent = {
  type: "message",
  channel: "D123",
  user: "u1",
  ts: "1700000000.000100",
  files: [
    {
      name: "diagram.png",
      mimetype: "image/png",
      url_private_download: "https://files.slack.com/files-pri/T1/diagram.png",
      size: 2048,
    },
    {
      name: "spec.pdf",
      mimetype: "application/pdf",
      url_private_download: "https://files.slack.com/files-pri/T1/spec.pdf",
    },
  ],
};

describe("extractSlackMedia", () => {
  it("surfaces image files with their download URLs", () => {
    const refs = extractSlackMedia(fileEvent);
    expect(refs).toEqual([
      {
        url: "https://files.slack.com/files-pri/T1/diagram.png",
        mimeType: "image/png",
        name: "diagram.png",
      },
    ]);
  });

  it("returns empty for non-file events", () => {
    expect(extractSlackMedia({ type: "message", text: "hi" })).toEqual([]);
    expect(extractSlackMedia(null)).toEqual([]);
  });
});

describe("normalizeSlackEvent with media (B4)", () => {
  it("accepts file-only messages (empty text)", () => {
    const result = normalizeSlackEvent({ type: "event_callback", event: fileEvent });
    expect(result.kind).toBe("message");
    if (result.kind === "message") {
      expect(result.message.text).toBe("");
      expect(result.message.isDirect).toBe(true);
    }
  });

  it("still ignores empty messages without files", () => {
    const result = normalizeSlackEvent({
      type: "event_callback",
      event: { type: "message", channel: "C1", user: "u", ts: "1" },
    });
    expect(result.kind).toBe("ignore");
  });
});
