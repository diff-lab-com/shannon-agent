/**
 * 修正1（帧契约评审定稿 2026-10-05，mobile 仓
 * `docs/frame-contract-review-2026-10-05.md` §2.1）：desktop 侧的**期望态**
 * 持久化 + 控制链路（重）连对账 —— `shannon/push.register {enable:false}` 与
 * `shannon/device.revoke` 从本地 no-op 变为转达 relay 的 `push.unbind`，且
 * bind/unbind 的用户意图落盘，链路重连时**末态制胜**地重申。
 *
 * 设计（评审钉定）：每 deviceId 记 `{enabled, platform?, token?}`（token 为
 * 最近一次注册值，加密落盘）；控制链路建立/重连时对账——期望开 → `push.bind`、
 * 期望关 → `push.unbind`。对账幂等、无动作队列：「先关后开 / 先开后关」的断链
 * 序列由最终对账收敛（评审因此否决待注销标记队列的乱序竞态）。附带自愈：relay
 * 侧绑定丢失（密钥丢失/换库）后，链路重连对账自动重建全部绑定。
 *
 * 落盘纪律与 relay 侧一致（AES-256-GCM，密钥首启自生成 0600；directE2E.ts
 * 模式）——桌面本就是用户信任根，但 token 密文与明文都不出现在日志。
 */

import { createCipheriv, createDecipheriv, randomBytes } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync, chmodSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

import type { Logger } from "../../adapters/types.js";
import { ShannonError } from "../protocol.js";
import { PushRelayBinding, type PushBindingRequest } from "./pushRelayBinding.js";

/** `~/.shannon/mobile-push-state/{expected.json,key}`。 */
export function pushExpectedStatePaths(baseDir?: string): { statePath: string; keyPath: string } {
  const dir = baseDir ?? join(homedir(), ".shannon", "mobile-push-state");
  return { statePath: join(dir, "expected.json"), keyPath: join(dir, "key") };
}

/** One device's push intent — the reconcile source of truth (末态). */
export interface PushExpectedEntry {
  enabled: boolean;
  /** Last registered platform; kept for the reconcile re-bind. */
  platform?: "fcm" | "apns";
  /** Last registered vendor token; kept (encrypted at rest) for the re-bind. */
  token?: string;
}

/** Encrypted-at-rest envelope (AES-256-GCM, random 12-byte IV per write). */
interface SealedState {
  v: 1;
  alg: "aes-256-gcm";
  iv: string;
  tag: string;
  data: string;
}

export interface PushExpectedStateStoreOptions {
  logger: Logger;
  /** Omit for the in-memory mode (tests / relay host mode off). */
  filePath?: string;
  /** Required with filePath; auto-generated (32 random bytes, 0600) if absent. */
  keyPath?: string;
}

export class PushExpectedStateStore {
  private readonly entries = new Map<string, PushExpectedEntry>();
  private readonly logger: Logger;
  private readonly filePath?: string;
  private readonly keyPath?: string;

  constructor(opts: PushExpectedStateStoreOptions) {
    this.logger = opts.logger;
    this.filePath = opts.filePath;
    this.keyPath = opts.keyPath;
    if (this.filePath) this.load();
  }

  get(deviceId: string): PushExpectedEntry | undefined {
    return this.entries.get(deviceId);
  }

  /** 用户开推送：意图先行记录（bind 失败也记——对账在链路恢复时自动补挂）。 */
  recordEnabled(deviceId: string, platform: "fcm" | "apns", token: string): void {
    this.entries.set(deviceId, { enabled: true, platform, token });
    this.persist();
  }

  /** 用户关推送：保留最近注册值（token 为注释性残留），enabled 决定对账方向。 */
  recordDisabled(deviceId: string): void {
    const prev = this.entries.get(deviceId);
    this.entries.set(deviceId, { enabled: false, platform: prev?.platform, token: prev?.token });
    this.persist();
  }

  /** §M2 吊销级联：设备已不存在，期望态整体抹除（unbind 尽力而为）。 */
  forget(deviceId: string): void {
    if (this.entries.delete(deviceId)) this.persist();
  }

  list(): Array<[string, PushExpectedEntry]> {
    return [...this.entries];
  }

  // ── persistence (0600, AES-256-GCM; corrupt → fresh, self-healing) ────────

  private load(): void {
    if (!this.filePath || !this.keyPath) return;
    // Read-only on boot: a missing/corrupt key leaves the (unreadable) state
    // unloaded and the store empty — persist() regenerates the key on the
    // first recorded intent (手机下次开推送重注册即重建，评审修正1的自愈路径).
    let key: Buffer;
    try {
      key = Buffer.from(readFileSync(this.keyPath, "utf8").trim(), "base64");
    } catch {
      return;
    }
    if (!existsSync(this.filePath)) return;
    try {
      const sealed = JSON.parse(readFileSync(this.filePath, "utf8")) as SealedState;
      const decipher = createDecipheriv("aes-256-gcm", key, Buffer.from(sealed.iv, "base64"));
      decipher.setAuthTag(Buffer.from(sealed.tag, "base64"));
      const plain = Buffer.concat([decipher.update(Buffer.from(sealed.data, "base64")), decipher.final()]);
      const parsed = JSON.parse(plain.toString("utf8")) as Record<string, PushExpectedEntry>;
      for (const [deviceId, entry] of Object.entries(parsed)) {
        if (entry && typeof entry.enabled === "boolean") this.entries.set(deviceId, entry);
      }
    } catch (err) {
      // 密钥丢失/文件损坏 = 期望态不可读（与 relay 侧同一 posture）：重新开始，
      // 手机下次开推送重注册即重建（评审修正1的自愈路径）。
      this.logger.warn(
        `push expected state unreadable — starting fresh (${(err as Error).message})`,
      );
      this.entries.clear();
    }
  }

  private writeKey(): Buffer {
    const key = randomBytes(32);
    mkdirSync(dirname(this.keyPath!), { recursive: true });
    writeFileSync(this.keyPath!, key.toString("base64"), { encoding: "utf8", mode: 0o600 });
    chmodSync(this.keyPath!, 0o600);
    return key;
  }

  private persist(): void {
    if (!this.filePath || !this.keyPath) return;
    try {
      const key = existsSync(this.keyPath)
        ? Buffer.from(readFileSync(this.keyPath, "utf8").trim(), "base64")
        : this.writeKey();
      const iv = randomBytes(12);
      const cipher = createCipheriv("aes-256-gcm", key, iv);
      const plain = Buffer.from(
        JSON.stringify(Object.fromEntries(this.entries)),
        "utf8",
      );
      // Ciphertext BEFORE the literal: getAuthTag() is only legal post-final().
      const ciphertext = Buffer.concat([cipher.update(plain), cipher.final()]);
      const sealed: SealedState = {
        v: 1,
        alg: "aes-256-gcm",
        iv: iv.toString("base64"),
        tag: cipher.getAuthTag().toString("base64"),
        data: ciphertext.toString("base64"),
      };
      mkdirSync(dirname(this.filePath), { recursive: true });
      writeFileSync(this.filePath, JSON.stringify(sealed), { encoding: "utf8", mode: 0o600 });
      chmodSync(this.filePath, 0o600);
    } catch (err) {
      // 落盘失败不挡注册主路——内存期望态仍在，本次进程内对账不受影响。
      this.logger.warn(`push expected state persist failed: ${(err as Error).message}`);
    }
  }
}

// ── reconciliation（评审 3.2：期望开 → push.bind；期望关 → push.unbind）──────

/**
 * Re-assert every recorded intent over the CURRENT control link. Idempotent
 * and last-state-wins: each device's intent is re-read AT ACTION TIME, so a
 * user toggle landing mid-reconcile flips that device's assertion instead of
 * racing it. Per-device best-effort — one refusal never starves the rest; the
 * next `host_ready` re-asserts whatever remains.
 */
export async function reconcileExpectedPushState(
  store: PushExpectedStateStore,
  binding: PushRelayBinding,
  logger: Logger,
): Promise<void> {
  for (const [deviceId] of store.list()) {
    const entry = store.get(deviceId); // re-read at action time — 末态制胜
    if (!entry) continue; // revoked while earlier devices were in flight
    try {
      if (entry.enabled && entry.platform && entry.token) {
        await binding.bind(deviceId, { platform: entry.platform, token: entry.token } satisfies PushBindingRequest);
      } else {
        await binding.unbind(deviceId);
      }
    } catch (err) {
      logger.warn(
        `push reconcile failed for ${deviceId} (${entry.enabled ? "bind" : "unbind"}): ` +
          `${(err as Error).message}`,
      );
    }
  }
}

// ── wiring factory（bootstrap 组装根：sinks + revoke 级联 + 对账挂点）────────

export interface PushWiring {
  /** §O2 注册腿：意图先行落盘，再尽力转发 push.bind（回 handle 给手机）。 */
  pushBindingSink: (deviceId: string, binding: PushBindingRequest) => Promise<{ handle: string }>;
  /** 修正1 注销腿：意图先行落盘，再尽力转发 push.unbind（不抛——对账兜底）。 */
  pushUnbindSink: (deviceId: string) => Promise<void>;
  /** §M2 吊销级联：抹期望态 + 尽力摘除 relay 绑定。 */
  onDeviceRevoked: (deviceId: string) => void;
  /** 控制链路（重）连挂点（relayHost `host_ready`）：重申全部期望态。 */
  reconcile: () => void;
}

export function createPushWiring(opts: {
  pushRelayRef: { current: PushRelayBinding | null };
  store: PushExpectedStateStore;
  logger: Logger;
}): PushWiring {
  const { pushRelayRef, store, logger } = opts;
  return {
    pushBindingSink: async (deviceId, binding) => {
      // Intent first: the user asked for push — a failed attempt (relay down,
      // vendor refusal) still reconciles on the next link establishment.
      store.recordEnabled(deviceId, binding.platform, binding.token);
      const relay = pushRelayRef.current;
      if (!relay) {
        throw Object.assign(
          new Error("push relay not connected (relay host mode disabled)"),
          { code: ShannonError.NOT_IMPLEMENTED },
        );
      }
      return relay.bind(deviceId, binding);
    },
    pushUnbindSink: async (deviceId) => {
      store.recordDisabled(deviceId);
      const relay = pushRelayRef.current;
      if (!relay) return; // 期望态已关；链路建立时的对账负责摘除
      await relay.unbind(deviceId);
    },
    onDeviceRevoked: (deviceId) => {
      store.forget(deviceId);
      const relay = pushRelayRef.current;
      if (!relay) return;
      relay.unbind(deviceId).catch((err: Error) => {
        logger.warn(`push unbind after revoke failed (reconcile will retry): ${err.message}`);
      });
    },
    reconcile: () => {
      const relay = pushRelayRef.current;
      if (!relay) return;
      void reconcileExpectedPushState(store, relay, logger);
    },
  };
}
