/**
 * Self-signed TLS for the mobile LAN face (v0.12 direct-mode hardening).
 *
 * The gateway binds 0.0.0.0 so phones on the LAN can reach it; plaintext ws
 * leaked chats/approvals to a passive MITM. TLS here uses a cert generated
 * ONCE on first boot and persisted under `~/.shannon/mobile-tls/`; trust is
 * established out-of-band: the QR embeds the cert's SHA-256 fingerprint and
 * the phone pins it (`onBadCertificate` comparison). Same trust model as the
 * pair token — the QR is the out-of-band channel.
 *
 * The desktop composes the LAN QR too, so the fingerprint is ALSO written to
 * `mobile-tls-info.json` next to the key (the Design-D file channel the
 * desktop already uses for pair tokens/devices).
 */

import { X509Certificate, createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { hostname } from "node:os";
import { dirname, join } from "node:path";

import selfsigned from "selfsigned";

export interface MobileTlsMaterial {
  /** PEM private key (0600 on disk). */
  key: string;
  /** PEM certificate. */
  cert: string;
  /** SHA-256 of the DER cert, lowercase hex, no colons (the QR field value). */
  fingerprint: string;
}

export interface MobileTlsInfoFile {
  /** Lowercase hex SHA-256 of the DER cert (matches the QR fingerprint). */
  fingerprint: string;
  /** Epoch ms the cert expires (825 days from generation). */
  expiresAt: number;
}

/** Default paths: `~/.shannon/mobile-tls/{key.pem,cert.pem,tls-info.json}`. */
export function mobileTlsPaths(baseDir?: string): {
  keyPath: string;
  certPath: string;
  infoPath: string;
} {
  const dir =
    baseDir ?? join(process.env.HOME ?? "~", ".shannon", "mobile-tls");
  return {
    keyPath: join(dir, "key.pem"),
    certPath: join(dir, "cert.pem"),
    infoPath: join(dir, "tls-info.json"),
  };
}

/** Lowercase colon-less SHA-256 fingerprint of a PEM cert (the QR form). */
export function certFingerprint(certPem: string): string {
  return new X509Certificate(certPem)
    .fingerprint256 // AA:BB:… colon-separated hex
    .replaceAll(":", "")
    .toLowerCase();
}

/**
 * Load the persisted TLS material, generating a fresh self-signed cert on
 * first use. Writes the info file for the desktop QR composer. Throws when
 * the key/cert can't be created or read — the caller surfaces it (fail loud:
 * silently falling back to plaintext would defeat the hardening).
 */
export function ensureTlsMaterial(paths = mobileTlsPaths()): MobileTlsMaterial {
  if (existsSync(paths.keyPath) && existsSync(paths.certPath)) {
    const key = readFileSync(paths.keyPath, "utf8");
    const cert = readFileSync(paths.certPath, "utf8");
    return { key, cert, fingerprint: certFingerprint(cert) };
  }

  const cn = `${hostname().split(".")[0] || "shannon"}.local`;
  const pems = selfsigned.generate([{ name: "commonName", value: cn }], {
    keySize: 2048,
    algorithm: "sha256",
    days: 825, // iOS ATS accepts ≤825-day certs
    extensions: [
      { name: "basicConstraints", cA: false },
      {
        name: "subjectAltName",
        altNames: [
          { type: 2, value: cn }, // DNS name
          { type: 2, value: "localhost" },
        ],
      },
    ],
  });

  mkdirSync(dirname(paths.keyPath), { recursive: true });
  writeFileSync(paths.keyPath, pems.private, { encoding: "utf8", mode: 0o600 });
  writeFileSync(paths.certPath, pems.cert, "utf8");

  const fingerprint = certFingerprint(pems.cert);
  const info: MobileTlsInfoFile = {
    fingerprint,
    expiresAt: Date.now() + 825 * 24 * 3600 * 1000,
  };
  writeFileSync(paths.infoPath, JSON.stringify(info, null, 2), {
    encoding: "utf8",
    mode: 0o600,
  });
  return { key: pems.private, cert: pems.cert, fingerprint };
}

/** SHA-256 fingerprint of arbitrary bytes (utility for tests/fixtures). */
export function sha256Hex(data: Buffer | string): string {
  return createHash("sha256").update(data).digest("hex");
}
