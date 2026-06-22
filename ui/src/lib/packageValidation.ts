// Strict validators for package-manager identifiers used when constructing
// stdio commands from MCP registry metadata. These prevent a malicious
// registry entry from sneaking flags or shell metacharacters into args.

const NPM_NAME = /^[a-zA-Z0-9][a-zA-Z0-9._-]*$/;
const NPM_SCOPED = /^@[a-zA-Z0-9][a-zA-Z0-9._-]*\/[a-zA-Z0-9][a-zA-Z0-9._-]*$/;
const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[a-zA-Z0-9.-]+)?(\+[a-zA-Z0-9.-]+)?$/;
const PY_NAME = /^[a-zA-Z0-9]([a-zA-Z0-9._-]*[a-zA-Z0-9])?$/;
const DOCKER_NAME = /^[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*(\/[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*)?(:[a-zA-Z0-9._-]+)?$/;

export type PkgKind = "npm" | "pip" | "docker";

export function isValidPackageName(kind: PkgKind, name: string): boolean {
  if (!name || name.length > 256) return false;
  switch (kind) {
    case "npm":
      return NPM_NAME.test(name) || NPM_SCOPED.test(name);
    case "pip":
      return PY_NAME.test(name);
    case "docker":
      return DOCKER_NAME.test(name);
  }
}

export function isValidVersion(kind: PkgKind, version: string): boolean {
  if (!version) return false;
  if (version.length > 64) return false;
  if (kind === "npm") return SEMVER.test(version) || /^[a-zA-Z0-9][a-zA-Z0-9._-]*$/.test(version);
  if (kind === "docker") return /^[a-zA-Z0-9._:-]+$/.test(version);
  return false;
}

export interface SafeUrlResult {
  ok: boolean;
  reason: "empty" | "invalid" | "scheme" | "private" | "ok";
}

const PRIVATE_HOST_PATTERNS = [
  /^localhost$/i,
  /^127\./,
  /^10\./,
  /^192\.168\./,
  /^172\.(1[6-9]|2\d|3[01])\./,
  /^169\.254\./,
  /^::1$/,
  /^fc00:/i,
  /^fe80:/i,
  /^fd/i,
  /\.local$/i,
  /^0\./,
];

export function validateWebhookUrl(raw: string): SafeUrlResult {
  const url = raw.trim();
  if (!url) return { ok: false, reason: "empty" };
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return { ok: false, reason: "invalid" };
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    return { ok: false, reason: "scheme" };
  }
  const host = parsed.hostname;
  if (PRIVATE_HOST_PATTERNS.some((re) => re.test(host))) {
    return { ok: false, reason: "private" };
  }
  return { ok: true, reason: "ok" };
}

export function safeErrorMessage(e: unknown, fallback: string): string {
  if (e instanceof Error) {
    const msg = e.message;
    if (/api[_-]?key|token|password|secret|bearer/i.test(msg)) return fallback;
    if (msg.length > 200) return fallback;
    return msg;
  }
  return fallback;
}
