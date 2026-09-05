import { describe, expect, it } from "vitest";

import { advertiseMobileServer } from "../mdns.js";
import { createConsoleLogger } from "../../logger.js";

const logger = createConsoleLogger("error");

// Lifecycle-only assertions: actually resolving the advertisement across the
// network is environment-dependent (multicast may be blocked in sandboxes),
// which would make the test flaky. What must hold everywhere is that publish
// does not throw synchronously and stop() is idempotent and resolves.
describe("advertiseMobileServer", () => {
  it("publishes and stops without crashing; stop is idempotent", async () => {
    const handle = advertiseMobileServer({ port: 33430, version: "test", logger });
    await expect(handle.stop()).resolves.toBeUndefined();
    await expect(handle.stop()).resolves.toBeUndefined();
  });
});
