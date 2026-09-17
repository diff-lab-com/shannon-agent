/**
 * Protocol schema contract (v0.12): the checked-in JSON schema
 * `docs/protocol/shannon-mobile-protocol.schema.json` is the cross-repo
 * reference the mobile repo pins against. This test forces the schema and the
 * gateway's runtime definitions (SHANNON_METHODS, ShannonError) to agree —
 * add a method/code in one place and the other test fails until both move.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { SHANNON_METHODS, ShannonError } from "../protocol.js";

const schemaPath = join(
  __dirname,
  "..",
  "..",
  "..",
  "..",
  "docs",
  "protocol",
  "shannon-mobile-protocol.schema.json",
);
const schema = JSON.parse(readFileSync(schemaPath, "utf8")) as {
  definitions: {
    shannonMethods: { enum: string[] };
    errorCodes: Record<string, number>;
    wireFixtures: { examples: unknown[] };
  };
};

describe("protocol schema contract", () => {
  it("schema method enum === SHANNON_METHODS (runtime + type single source)", () => {
    expect([...schema.definitions.shannonMethods.enum].sort()).toEqual(
      [...SHANNON_METHODS].sort(),
    );
  });

  it("schema error codes === ShannonError", () => {
    const codeProps = schema.definitions.errorCodes.properties as Record<string, number>;
    for (const [name, code] of Object.entries(codeProps)) {
      expect(ShannonError[name as keyof typeof ShannonError]).toBe(code);
    }
    // No extra codes on the TS side without a schema entry.
    expect(Object.keys(ShannonError).sort()).toEqual(Object.keys(codeProps).sort());
  });

  it("wire fixtures stay parseable (the mobile repo pins the same payloads)", () => {
    const fixtures = schema.definitions
      .wireFixtures.examples as unknown as Array<[string, unknown]>;
    expect(fixtures.length).toBeGreaterThanOrEqual(5);
    const resume = Object.fromEntries(fixtures)[
      "device.resume (nonce anti-replay)"
    ] as { nonce?: string };
    expect(typeof resume.nonce).toBe("string");
    const skew = Object.fromEntries(fixtures)["clock-skew rejection"] as {
      error: { code: number };
    };
    expect(skew.error.code).toBe(ShannonError.CLOCK_SKEW);
  });
});
