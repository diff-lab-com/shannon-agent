import {
  mkdirSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

import { type Platform } from "../adapters/types.js";

/**
 * Persisted allowlist of who may drive the agent (F14 access control).
 *
 * Keyed by `{platform, senderId}` — pairing approves a *person*, who can then
 * use the bot in any conversation. Stored as JSON at the given path (atomic
 * tmp+rename); pass `undefined` for an in-memory allowlist (tests).
 *
 * The store is the gateway's source of truth; nothing reads credentials here
 * (those live in the OS keyring — see AdapterContext.getSecret).
 */

/** Default persistence path (review F42): `~/.shannon/gateway/allowlist.json`. */
export function defaultAllowlistPath(): string {
  return join(homedir(), ".shannon", "gateway", "allowlist.json");
}

/**
 * Resolve the allowlist file path (review F42): explicit argument (pass
 * `null` to force the in-memory store, i.e. tests) >
 * `$SHANNON_GATEWAY_ALLOWLIST` > `~/.shannon/gateway/allowlist.json`.
 */
export function resolveAllowlistPath(explicit?: string | null): string | undefined {
  if (explicit === null) return undefined;
  if (explicit !== undefined) return explicit;
  const fromEnv = process.env.SHANNON_GATEWAY_ALLOWLIST;
  if (fromEnv) return fromEnv;
  return defaultAllowlistPath();
}

export interface AllowlistEntry {
  platform: Platform;
  senderId: string;
  /** Epoch ms when the entry was added. */
  addedAt: number;
}

interface AllowlistFile {
  entries: AllowlistEntry[];
}

export class Allowlist {
  private readonly entries = new Map<string, AllowlistEntry>();

  constructor(private readonly filePath?: string) {
    if (filePath) this.load();
  }

  allow(platform: Platform, senderId: string, addedAt: number = Date.now()): void {
    const entry: AllowlistEntry = { platform, senderId, addedAt };
    this.entries.set(key(platform, senderId), entry);
    this.persist();
  }

  isAllowed(platform: Platform, senderId: string): boolean {
    return this.entries.has(key(platform, senderId));
  }

  revoke(platform: Platform, senderId: string): boolean {
    const removed = this.entries.delete(key(platform, senderId));
    if (removed) this.persist();
    return removed;
  }

  get size(): number {
    return this.entries.size;
  }

  /** Persistence path, or undefined for the in-memory (tests) store. */
  get file(): string | undefined {
    return this.filePath;
  }

  list(): AllowlistEntry[] {
    return [...this.entries.values()];
  }

  private load(): void {
    if (!this.filePath) return;
    let raw: string;
    try {
      raw = readFileSync(this.filePath, "utf8");
    } catch {
      return; // missing file = empty allowlist (first run)
    }
    try {
      const parsed = JSON.parse(raw) as AllowlistFile;
      if (parsed?.entries && Array.isArray(parsed.entries)) {
        for (const e of parsed.entries) {
          this.entries.set(key(e.platform, e.senderId), e);
        }
      }
    } catch {
      // Corrupt file — start empty rather than crashing the gateway.
    }
  }

  private persist(): void {
    if (!this.filePath) return;
    const payload: AllowlistFile = { entries: this.list() };
    const tmp = `${this.filePath}.tmp`;
    mkdirSync(dirname(this.filePath), { recursive: true });
    writeFileSync(tmp, JSON.stringify(payload, null, 2), "utf8");
    renameSync(tmp, this.filePath);
  }
}

function key(platform: Platform, senderId: string): string {
  return `${platform}:${senderId}`;
}
