/**
 * mDNS (Bonjour/zeroconf) advertisement for the mobile pairing server.
 *
 * cross-repo-adaptation-spec §A8/§A8b: iOS App Transport Security does NOT
 * cover raw LAN IPs — `NSAllowsLocalNetworking` only permits `.local` (mDNS)
 * and link-local hosts, and iOS rejects `ws://192.168.x.x` before any Shannon
 * code runs — so the LAN direct-connect track requires the desktop to be
 * reachable under a `.local` hostname. Publishing `_shannon._tcp` with the
 * bound pairing port is that scenario's hard prerequisite, not an optional
 * optimization. The phone never browses (it connects to the hostname the QR
 * carries), so the mobile side needs no `NSBonjourServices` declaration.
 */

import { randomUUID } from "node:crypto";
import { hostname } from "node:os";

import { Bonjour } from "bonjour-service";

import type { Logger } from "../adapters/types.js";

export interface MdnsAdvertisementOptions {
  /** Port the mobile pairing WS server actually bound (may be ephemeral). */
  port: number;
  /** Gateway version surfaced in the TXT record. */
  version: string;
  logger: Logger;
}

export interface MdnsHandle {
  /** Unpublish the service and release the mDNS sockets. Idempotent. */
  stop(): Promise<void>;
}

export function advertiseMobileServer(opts: MdnsAdvertisementOptions): MdnsHandle {
  const instanceId = randomUUID();
  const bonjour = new Bonjour({}, (err: Error) => {
    opts.logger.warn(`mobile mDNS: responder error: ${err.message}`);
  });
  const service = bonjour.publish({
    name: `Shannon on ${hostname()}`,
    type: "shannon",
    protocol: "tcp",
    port: opts.port,
    txt: {
      instanceId,
      version: opts.version,
      port: String(opts.port),
    },
  });
  service.on("error", (err: Error) => {
    opts.logger.warn(`mobile mDNS: advertisement error: ${err.message}`);
  });
  opts.logger.info(
    `mobile mDNS: advertising ${service.fqdn} (port ${opts.port}, instanceId ${instanceId})`,
  );

  let stopped = false;
  return {
    stop(): Promise<void> {
      if (stopped) return Promise.resolve();
      stopped = true;
      return new Promise((resolve) => {
        try {
          bonjour.unpublishAll(() => {
            bonjour.destroy(() => resolve());
          });
        } catch {
          // The responder can already be gone (interface flap, shutdown race);
          // stop() must never reject the bootstrap shutdown path.
          resolve();
        }
      });
    },
  };
}
