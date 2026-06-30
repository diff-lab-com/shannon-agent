/**
 * shannon-gateway entry point.
 *
 * Phase 1 scaffold: the four layers (transport adapters → normalizer →
 * session router → engine client) are populated by P1-a…P1-f. For now this
 * module only reports status so `pnpm dev` is a no-op rather than a mystery.
 */

export const GATEWAY_VERSION = "0.1.0";

function main(): void {
  console.log(
    `shannon-gateway ${GATEWAY_VERSION} — scaffold. ` +
      `Engine client (P1-a) not yet wired.`,
  );
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main();
}
