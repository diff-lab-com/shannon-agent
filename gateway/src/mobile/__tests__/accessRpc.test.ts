import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { Allowlist } from "../../access/allowlist.js";
import { AllowlistGuard } from "../../access/guard.js";
import { PairingStore } from "../../access/pairing.js";
import type { NormalizedInbound } from "../../adapters/types.js";
import { createConsoleLogger } from "../../logger.js";
import { createPairingAccess, PAIRING_APPROVE_PATH, PAIRING_PENDING_PATH } from "../accessRpc.js";
import { DeviceRegistry, PairTokenStore } from "../pairing.js";
import { ShannonError } from "../protocol.js";
import { MobileServer, type MethodContext } from "../server.js";

const logger = createConsoleLogger("error");

const dirs: string[] = [];
afterEach(() => {
  while (dirs.length > 0) {
    const dir = dirs.pop();
    if (dir) rmSync(dir, { recursive: true, force: true });
  }
});

function tmpDir(): string {
  const dir = mkdtempSync(join(tmpdir(), "gw-access-rpc-"));
  dirs.push(dir);
  return dir;
}

function ctx(sessionId: string | null): MethodContext {
  return { socket: null as unknown as MethodContext["socket"], sessionId, logger };
}

function dm(platform: string, senderId: string): NormalizedInbound {
  return {
    platform: platform as NormalizedInbound["platform"],
    chatId: "C1",
    senderId,
    senderName: senderId,
    text: "let me in",
    timestamp: Date.now(),
    isDirect: true,
  };
}

/** Issue a challenge through the real guard so RPC and IM share one store. */
function issueViaGuard(
  allowlist: Allowlist,
  pairing: PairingStore,
  platform: string,
  senderId: string,
): { code: string; expiresAt: number } {
  const guard = new AllowlistGuard(allowlist, pairing);
  const decision = guard.check(dm(platform, senderId));
  if (decision.decision !== "challenge") {
    throw new Error(`expected challenge, got ${decision.decision}`);
  }
  return { code: decision.code, expiresAt: decision.expiresAt };
}

interface AccessFixture {
  handlers: ReturnType<typeof createPairingAccess>["handlers"];
  http: ReturnType<typeof createPairingAccess>["http"];
  allowlist: Allowlist;
  pairing: PairingStore;
  tokens: PairTokenStore;
}

function fixture(opts?: { pairingTtlMs?: number; now?: () => number }): AccessFixture {
  const allowlist = new Allowlist();
  const pairing = new PairingStore(opts?.pairingTtlMs, opts?.now);
  const tokens = new PairTokenStore();
  const access = createPairingAccess({
    allowlist,
    pairing,
    tokens,
    registry: new DeviceRegistry(),
    logger,
  });
  return { ...access, allowlist, pairing, tokens };
}

describe("createPairingAccess handlers (T9)", () => {
  it("shannon/pairing.pending requires a device session or a pair token", async () => {
    const access = fixture();
    const noSession = await access.handlers["shannon/pairing.pending"]!({}, ctx(null));
    expect(noSession.kind).toBe("error");
    expect((noSession as { code: number }).code).toBe(ShannonError.PAIRING_REQUIRED);
  });

  it("pending lists issued challenges (token leg) without consuming the token", async () => {
    const dir = tmpDir();
    const allowlist = new Allowlist();
    const pairing = new PairingStore();
    // File-backed like production: the desktop mints into the JSONL.
    const tokens = new PairTokenStore({ filePath: join(dir, "tokens.jsonl") });
    const access = createPairingAccess({
      allowlist,
      pairing,
      tokens,
      registry: new DeviceRegistry(),
      logger,
    });
    const token = tokens.issue().token;
    issueViaGuard(allowlist, pairing, "slack", "U1");

    const first = (await access.handlers["shannon/pairing.pending"]!({ token }, ctx(null))) as {
      kind: "result";
      result: { pending: Array<Record<string, unknown>> };
    };
    expect(first.kind).toBe("result");
    expect(first.result.pending).toHaveLength(1);
    expect(first.result.pending[0]).toMatchObject({
      platform: "slack",
      senderId: "U1",
      code: expect.stringMatching(/^\d{6}$/),
      requestedAt: expect.any(Number),
      expiresAt: expect.any(Number),
    });

    // Read-only: the same token still verifies on a refresh.
    const second = (await access.handlers["shannon/pairing.pending"]!({ token }, ctx(null))) as {
      kind: "result";
    };
    expect(second.kind).toBe("result");
  });

  it("pending works over a bound trusted device session, rejects a revoked one", async () => {
    const registry = new DeviceRegistry();
    const device = registry.upsert("device-a", "pk");
    const access = createPairingAccess({
      allowlist: new Allowlist(),
      pairing: new PairingStore(),
      tokens: new PairTokenStore(),
      registry,
      logger,
    });
    const ok = await access.handlers["shannon/pairing.pending"]!({}, ctx("device-a"));
    expect(ok.kind).toBe("result");

    registry.revoke(device.device_id);
    const revoked = await access.handlers["shannon/pairing.pending"]!({}, ctx("device-a"));
    expect(revoked.kind).toBe("error");
    expect((revoked as { code: number }).code).toBe(ShannonError.PAIRING_REQUIRED);
  });

  it("pending prunes expired challenges instead of listing them", async () => {
    let now = 1_000_000;
    const access = fixture({ pairingTtlMs: 5 * 60_000, now: () => now });
    const token = access.tokens.issue().token;
    issueViaGuard(access.allowlist, access.pairing, "slack", "U1");
    now += 6 * 60_000; // past the TTL
    const listed = (await access.handlers["shannon/pairing.pending"]!({ token }, ctx(null))) as {
      kind: "result";
      result: { pending: unknown[] };
    };
    expect(listed.result.pending).toHaveLength(0);
  });

  it("shannon/pairing.approve consumes the code, persists the allowlist, and burns the token", async () => {
    const dir = tmpDir();
    const allowlistPath = join(dir, "allowlist.json");
    const allowlist = new Allowlist(allowlistPath);
    const pairing = new PairingStore();
    const tokens = new PairTokenStore({ filePath: join(dir, "tokens.jsonl") });
    const access = createPairingAccess({ allowlist, pairing, tokens, registry: new DeviceRegistry(), logger });
    const { code } = issueViaGuard(allowlist, pairing, "slack", "U1");
    const token = tokens.issue().token;

    const outcome = (await access.handlers["shannon/pairing.approve"]!({ code, token }, ctx(null))) as {
      kind: "result";
      result: { ok: true; record: { code: string; platform: string; senderId: string } };
    };
    expect(outcome.kind).toBe("result");
    expect(outcome.result.record).toMatchObject({ code, platform: "slack", senderId: "U1" });
    expect(allowlist.isAllowed("slack", "U1")).toBe(true);
    // Persisted via the same Allowlist.allow the IM reply uses.
    const persisted = JSON.parse(readFileSync(allowlistPath, "utf8")) as {
      entries: Array<{ platform: string; senderId: string }>;
    };
    expect(persisted.entries).toContainEqual({
      platform: "slack",
      senderId: "U1",
      addedAt: expect.any(Number),
    });

    // Single-use: the burned token can't approve a second pairing.
    const { code: code2 } = issueViaGuard(allowlist, pairing, "slack", "U2");
    const replay = await access.handlers["shannon/pairing.approve"]!({ code: code2, token }, ctx(null));
    expect(replay.kind).toBe("error");
    expect((replay as { code: number }).code).toBe(ShannonError.PAIRING_REQUIRED);
    expect(allowlist.isAllowed("slack", "U2")).toBe(false);
  });

  it("approve shares ONE implementation with the IM `approve <code>` reply (same stores)", async () => {
    const access = fixture();
    const guard = new AllowlistGuard(access.allowlist, access.pairing);

    // Issue via the guard (IM path), approve via RPC → the IM path sees the
    // sender as allowed.
    const { code } = issueViaGuard(access.allowlist, access.pairing, "telegram", "U77");
    const rpcOutcome = (await access.handlers["shannon/pairing.approve"]!(
      { code, token: access.tokens.issue().token },
      ctx(null),
    )) as { kind: "result" };
    expect(rpcOutcome.kind).toBe("result");
    expect(guard.check(dm("telegram", "U77")).decision).toBe("allow");

    // Reverse direction: the desktop sees an IM-issued challenge in pending,
    // then the paired sender's IM reply approves it — one store throughout.
    const second = issueViaGuard(access.allowlist, access.pairing, "telegram", "U88");
    const pending = (await access.handlers["shannon/pairing.pending"]!(
      { token: access.tokens.issue().token },
      ctx(null),
    )) as { kind: "result"; result: { pending: Array<{ code: string }> } };
    expect(pending.result.pending.map((p) => p.code)).toContain(second.code);
    const imOutcome = guard.approve(dm("telegram", "U77"), second.code);
    expect(imOutcome.ok).toBe(true);
    expect(access.allowlist.isAllowed("telegram", "U88")).toBe(true);
  });

  it("approve rejects unknown and expired codes with the guard's reason", async () => {
    const access = fixture();
    const token = access.tokens.issue().token;

    const unknown = await access.handlers["shannon/pairing.approve"]!({ code: "000000", token }, ctx(null));
    expect(unknown.kind).toBe("error");
    expect((unknown as { code: number }).code).toBe(ShannonError.BAD_PARAMS);
    expect((unknown as { message: string }).message).toMatch(/Unknown or expired pairing code/);
    expect(access.allowlist.isAllowed("slack", "U9")).toBe(false);

    let now = 1_000_000;
    const timed = fixture({ pairingTtlMs: 60_000, now: () => now });
    const { code } = issueViaGuard(timed.allowlist, timed.pairing, "slack", "U9");
    now += 61_000;
    const expired = await timed.handlers["shannon/pairing.approve"]!(
      { code, token: timed.tokens.issue().token },
      ctx(null),
    );
    expect(expired.kind).toBe("error");
    expect(timed.allowlist.isAllowed("slack", "U9")).toBe(false);
  });

  it("approve requires params.code", async () => {
    const access = fixture();
    const missing = await access.handlers["shannon/pairing.approve"]!(
      { token: access.tokens.issue().token },
      ctx(null),
    );
    expect(missing.kind).toBe("error");
    expect((missing as { code: number }).code).toBe(ShannonError.BAD_PARAMS);
    expect((missing as { message: string }).message).toMatch(/params\.code/);
  });
});

describe("createPairingAccess HTTP skin (T9 desktop leg)", () => {
  it("returns null for foreign paths so the server falls through to 404", async () => {
    const access = fixture();
    expect(await access.http("/rpc/pairing/other", "{}")).toBeNull();
    expect(await access.http("/", "{}")).toBeNull();
  });

  it("serves pending + approve over the fixed paths with JSON bodies", async () => {
    const dir = tmpDir();
    const allowlist = new Allowlist();
    const pairing = new PairingStore();
    const tokens = new PairTokenStore({ filePath: join(dir, "tokens.jsonl") });
    const access = createPairingAccess({ allowlist, pairing, tokens, registry: new DeviceRegistry(), logger });
    const { code } = issueViaGuard(allowlist, pairing, "slack", "U1");
    const token = tokens.issue().token;

    const pendingRes = await access.http(PAIRING_PENDING_PATH, JSON.stringify({ token }));
    expect(pendingRes?.status).toBe(200);
    const pendingBody = JSON.parse(pendingRes!.body) as {
      result: { pending: Array<{ code: string; senderId: string }> };
    };
    expect(pendingBody.result.pending.map((p) => p.code)).toContain(code);

    const approveRes = await access.http(
      PAIRING_APPROVE_PATH,
      JSON.stringify({ token: tokens.issue().token, code }),
    );
    expect(approveRes?.status).toBe(200);
    const approveBody = JSON.parse(approveRes!.body) as {
      result: { ok: true; record: { senderId: string } };
    };
    expect(approveBody.result.record.senderId).toBe("U1");
    expect(allowlist.isAllowed("slack", "U1")).toBe(true);
  });

  it("maps handler errors to 400 {error:{code,message}} and bad JSON to PARSE_ERROR", async () => {
    const access = fixture();
    const unauthorized = await access.http(PAIRING_PENDING_PATH, "{}");
    expect(unauthorized?.status).toBe(400);
    const unauthorizedBody = JSON.parse(unauthorized!.body) as { error: { code: number } };
    expect(unauthorizedBody.error.code).toBe(ShannonError.PAIRING_REQUIRED);

    const badJson = await access.http(PAIRING_PENDING_PATH, "{not json");
    expect(badJson?.status).toBe(400);
    const badJsonBody = JSON.parse(badJson!.body) as { error: { code: number } };
    expect(badJsonBody.error.code).toBe(ShannonError.PARSE_ERROR);
  });
});

describe("MobileServer POST plumbing (T9)", () => {
  it("routes POST /rpc/pairing/* to the httpApi and drops foreign-origin POSTs", async () => {
    const access = fixture();
    const token = access.tokens.issue().token;
    const server = new MobileServer({
      host: "127.0.0.1",
      port: 0,
      logger,
      handlers: {},
      httpApi: access.http,
    });
    const handle = await server.start();
    const base = `http://127.0.0.1:${handle.port}`;

    const ok = await fetch(`${base}${PAIRING_PENDING_PATH}`, {
      method: "POST",
      body: JSON.stringify({ token }),
    });
    expect(ok.status).toBe(200);
    const body = (await ok.json()) as { result: { pending: unknown[] } };
    expect(Array.isArray(body.result.pending)).toBe(true);

    // Cross-site POST (a browser page on another origin) is dropped like WS
    // upgrades — here it falls through to a bare 404.
    const crossSite = await fetch(`${base}${PAIRING_PENDING_PATH}`, {
      method: "POST",
      headers: { origin: "https://evil.example" },
      body: "{}",
    });
    expect(crossSite.status).toBe(404);

    // GET never reaches the JSON endpoints.
    const get = await fetch(`${base}${PAIRING_PENDING_PATH}`);
    expect(get.status).toBe(404);

    await handle.stop();
  });
});
