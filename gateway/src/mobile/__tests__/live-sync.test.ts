/**
 * WP-15 T3/T4 acceptance tests — device revocation + live-sync surface.
 *
 * Contract source: shannon-mobile `lib/src/live/live_sync.dart` +
 * `mock_server.dart` (the phone reads only `lastSeq` from snapshot/resume;
 * push notifications carry a top-level monotonic `seq`; gapTooLarge = -32014).
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { WebSocket } from "ws";

import { deviceIdFromPublicKey, generateEd25519KeyPair, pairPopMessage, signMessage } from "../crypto.js";
import { createConsoleLogger } from "../../logger.js";
import { sharedPushSeq, GAP_WINDOW } from "../seq.js";
import { bootstrap } from "../../bootstrap.js";
import { mockEngineClient } from "./dispatch.test.js";

const logger = createConsoleLogger("error");
const dirs: string[] = [];

afterEach(() => {
  while (dirs.length) rmSync(dirs.pop()!, { recursive: true, force: true });
  vi.restoreAllMocks();
});

interface Harness {
  port: number;
  rpc: (method: string, params?: unknown) => Promise<any>;
  close: () => void;
  socket: WebSocket;
  devicesFile: string;
  deviceId: string;
}

/** boot → pair → bound socket. Returns an rpc helper over the live socket. */
async function harness(opts?: { engineEvents?: number }): Promise<Harness> {
  const dir = mkdtempSync(join(tmpdir(), "gw-livesync-"));
  dirs.push(dir);
  const tokensFile = join(dir, "tokens.jsonl");
  const devicesFile = join(dir, "devices.json");

  const kp = generateEd25519KeyPair();
  const token = "livesync-token";
  writeFileSync(
    tokensFile,
    JSON.stringify({ token, issuedAt: Date.now(), expiresAt: Date.now() + 75_000 }) + "\n",
    "utf8",
  );

  const client = mockEngineClient(
    Array.from({ length: opts?.engineEvents ?? 0 }, (_, i) => ({
      type: "text",
      content: `chunk-${i}`,
    }) as any),
  );

  const handle = await bootstrap(
    {
      engine: { wsUrl: "ws://mock/api/ws", httpBaseUrl: "http://mock" },
      adapters: [],
      mobile: { enabled: true, host: "127.0.0.1", port: 0, tokensFile, devicesFile },
    },
    { factories: new Map(), mobileEngineClientFactory: () => client, logger },
  );
  const port = handle.mobilePort!;
  const deviceId = deviceIdFromPublicKey(kp.publicKeyB64Url);

  const socket = new WebSocket(`ws://127.0.0.1:${port}/`);
  await new Promise<void>((resolve, reject) => {
    socket.once("open", () => resolve());
    socket.once("error", reject);
  });

  const rpc = (method: string, params: unknown = {}): Promise<any> =>
    new Promise((resolve) => {
      const id = Math.floor(Math.random() * 1e9);
      const onMsg = (data: unknown): void => {
        const msg = JSON.parse(String(data)) as any;
        if (msg.id === id) {
          socket.off("message", onMsg);
          if (msg.error) resolve({ __error: msg.error });
          else resolve(msg.result);
        }
      };
      socket.on("message", onMsg);
      socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
    });

  const pop = signMessage(kp.privateKey, pairPopMessage(token, kp.publicKeyB64Url));
  const pairRes = await rpc("shannon/pair", {
    pair_token: token,
    device_public_key: kp.publicKeyB64Url,
    pop_signature: pop,
    device_label: "livesync test",
  });
  expect(pairRes.device_id).toBe(deviceId);

  return {
    port,
    rpc,
    close: () => socket.close(),
    socket,
    devicesFile,
    deviceId,
  };
}

describe("WP-15 T4 — live-sync snapshot / resume / seq", () => {
  it("snapshot returns a positive lastSeq cursor", async () => {
    const h = await harness();
    try {
      const snap = await h.rpc("shannon/snapshot");
      expect(typeof snap.lastSeq).toBe("number");
      expect(snap.lastSeq).toBeGreaterThanOrEqual(0);
    } finally {
      h.close();
    }
  });

  it("resume inside the window reports the head cursor; beyond it is gapTooLarge", async () => {
    const h = await harness();
    try {
      // Inside the window (0 is always within GAP_WINDOW of a fresh counter).
      const ok = await h.rpc("shannon/resume", { sinceSeq: 0 });
      expect(ok.sinceSeq).toBe(0);
      expect(typeof ok.lastSeq).toBe("number");
      expect(ok.replayed).toEqual([]);

      // Cursor beyond the head (gateway restart reset the counter): the mock
      // contract treats this as a legal rewind — the phone adopts the
      // returned head cursor.
      const far = await h.rpc("shannon/resume", { sinceSeq: 1_000_000_000 });
      expect(far.sinceSeq).toBe(1_000_000_000);
      expect(far.lastSeq).toBeLessThan(1_000_000_000);

      // A genuinely large backward gap (head far ahead of the cursor) is
      // rejected so the phone re-snapshots. Drive the counter forward first.
      for (let i = 0; i < GAP_WINDOW + 5; i++) sharedPushSeq.next();
      const gapped = await h.rpc("shannon/resume", { sinceSeq: 0 });
      expect(gapped.__error.code).toBe(-32014);
      expect(gapped.__error.message).toMatch(/gap exceeds retained window/);
    } finally {
      h.close();
    }
  });

  it("stream + push notifications carry a monotonic top-level seq", async () => {
    const h = await harness({ engineEvents: 3 });
    const notifications: any[] = [];
    h.socket.on("message", (data: unknown) => {
      const msg = JSON.parse(String(data)) as any;
      if (msg.method === "shannon/event") notifications.push(msg.params);
    });
    try {
      await h.rpc("shannon/query", { prompt: "go" });
      await new Promise((r) => setTimeout(r, 400));
      const seqs = notifications.map((n) => n.seq);
      expect(seqs.length).toBeGreaterThanOrEqual(4); // started + 3 chunks
      for (let i = 1; i < seqs.length; i++) expect(seqs[i]).toBeGreaterThan(seqs[i - 1]);
    } finally {
      h.close();
    }
  });
});

describe("WP-15 T3 — device revocation bites on live sessions", () => {
  it("device.list shows the paired device; revoke then yields PAIRING_REQUIRED on every RPC", async () => {
    const h = await harness();
    try {
      const list = await h.rpc("shannon/device.list");
      expect(list.devices.map((d: any) => d.device_id)).toContain(h.deviceId);

      const rev = await h.rpc("shannon/device.revoke", { device_id: h.deviceId });
      expect(rev.revoked).toBe(true);

      // Acceptance: ANY subsequent RPC on the revoked session answers
      // PAIRING_REQUIRED (-32000).
      const q = await h.rpc("shannon/query", { prompt: "still there?" });
      expect(q.__error.code).toBe(-32000);
      const again = await h.rpc("shannon/device.list");
      expect(again.__error.code).toBe(-32000);
    } finally {
      h.close();
    }
  });

  it("an out-of-band desktop revoke (file rewrite) is picked up without a restart", async () => {
    const h = await harness();
    try {
      // Simulate the desktop's mobile_revoke_device: rewrite the registry
      // file directly, removing the device.
      writeFileSync(h.devicesFile, JSON.stringify({ entries: [] }), "utf8");

      // The registry refreshes from disk on read (mtime check).
      const q = await h.rpc("shannon/query", { prompt: "still there?" });
      expect(q.__error.code).toBe(-32000);
    } finally {
      h.close();
    }
  });

  it("device.revoke requires params and a paired session", async () => {
    const h = await harness();
    try {
      const missing = await h.rpc("shannon/device.revoke", {});
      expect(missing.__error.code).toBe(-32001);
    } finally {
      h.close();
    }
  });
});
