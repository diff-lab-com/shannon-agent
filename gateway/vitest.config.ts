import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
    // Tests may spin up real sockets / fake servers; give them room.
    testTimeout: 10_000,
  },
});
