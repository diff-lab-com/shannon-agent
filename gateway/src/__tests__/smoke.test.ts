import { describe, it, expect } from "vitest";

import { GATEWAY_VERSION } from "../index.js";

describe("scaffold smoke", () => {
  it("exposes a version string", () => {
    expect(typeof GATEWAY_VERSION).toBe("string");
    expect(GATEWAY_VERSION.length).toBeGreaterThan(0);
  });
});
