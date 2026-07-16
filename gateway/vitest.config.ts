import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
    // Tests may spin up real sockets / fake servers; give them room.
    testTimeout: 10_000,
    // Run tests in a single fork, sequentially, so tests that share global
    // state (engine wsClient, mobile server port, env vars) cannot leak
    // state into each other. CI runners see this race more than local
    // hardware, so the isolation matters most in CI.
    pool: "forks",
    poolOptions: {
      forks: {
        singleFork: true,
      },
    },
  },
});
