// Pure helpers used by the McpAddServerDialog tabs.
// Kept identical to the prior McpServers.tsx implementation.

import {
  isValidPackageName,
  isValidVersion,
} from "@/lib/packageValidation";
import type { StdioMcpSpecPayload } from "@/lib/tauri-api";
import type {
  ParsedMcpServer,
  RegistryPackage,
} from "./types";

// --- Registry → stdio spec (npm/pip/docker) --------------------------------

export function buildSpecFromPackage(
  serverName: string,
  pkg: RegistryPackage | null | undefined,
): StdioMcpSpecPayload | null {
  if (!pkg) return null;
  const name = pkg.name?.trim();
  const version = pkg.version?.trim();
  const versionSuffix = version ? `@${version}` : "";
  switch (pkg.kind) {
    case "npm": {
      if (!name || !isValidPackageName("npm", name)) return null;
      if (version && !isValidVersion("npm", version)) return null;
      return {
        server_name: serverName,
        command: "npx",
        args: ["-y", versionSuffix ? `${name}${versionSuffix}` : name],
        env: [],
      };
    }
    case "pip": {
      if (!name || !isValidPackageName("pip", name)) return null;
      return {
        server_name: serverName,
        command: "uvx",
        args: [name],
        env: [],
      };
    }
    case "docker": {
      if (!name || !isValidPackageName("docker", name)) return null;
      if (version && !isValidVersion("docker", version)) return null;
      return {
        server_name: serverName,
        command: "docker",
        args: ["run", "-i", "--rm", name],
        env: [],
      };
    }
    default:
      return null;
  }
}

export function packageManagerLabel(
  pkg: RegistryPackage | null | undefined,
): string | null {
  if (!pkg) return null;
  switch (pkg.kind) {
    case "npm":
      return "npx";
    case "pip":
      return "uvx";
    case "docker":
      return "docker";
    default:
      return null;
  }
}

// --- JSON paste parsing ----------------------------------------------------

/**
 * Parse pasted JSON in either Cursor (`{ mcpServers: { ... } }`) or
 * single-server shape (`{ command, args?, env? }`). Returns a list of
 * servers to install. Throws on malformed input or missing `command`.
 */
export function parseMcpJson(raw: string): ParsedMcpServer[] {
  const trimmed = raw.trim();
  if (!trimmed) throw new Error("empty input");
  let data: unknown;
  try {
    data = JSON.parse(trimmed);
  } catch (e) {
    throw new Error(e instanceof Error ? e.message : "invalid JSON");
  }
  if (typeof data !== "object" || data === null) {
    throw new Error("JSON root must be an object");
  }

  // Helper: normalise a single server definition into ParsedMcpServer.
  const normalise = (
    name: string,
    def: unknown,
    fallbackName?: string,
  ): ParsedMcpServer | null => {
    if (typeof def !== "object" || def === null) return null;
    const obj = def as Record<string, unknown>;
    const command = typeof obj.command === "string" ? obj.command.trim() : "";
    if (!command) return null;
    const serverName = (name || fallbackName || "Custom Server").trim();
    const args = Array.isArray(obj.args)
      ? obj.args.filter((a) => typeof a === "string").map((a) => String(a))
      : [];
    const env: [string, string][] = [];
    if (obj.env && typeof obj.env === "object") {
      for (const [k, v] of Object.entries(obj.env as Record<string, unknown>)) {
        if (typeof v === "string" || typeof v === "number") {
          env.push([k, String(v)]);
        }
      }
    }
    return { name: serverName, command, args, env };
  };

  const root = data as Record<string, unknown>;

  // Cursor format: { "mcpServers": { "name": { ... }, ... } }
  if (root.mcpServers && typeof root.mcpServers === "object") {
    const entries = Object.entries(root.mcpServers as Record<string, unknown>);
    const out: ParsedMcpServer[] = [];
    for (const [serverName, def] of entries) {
      const parsed = normalise(serverName, def);
      if (parsed) out.push(parsed);
    }
    if (out.length === 0) {
      throw new Error("no valid servers in mcpServers");
    }
    return out;
  }

  // Claude Desktop format: { "mcpServers": { ... } } is the same as Cursor,
  // but Claude Desktop config files sometimes nest under a different key or
  // present a single server at the root. Handle single-server-at-root too.
  if (typeof root.command === "string") {
    const parsed = normalise("", root, "Custom Server");
    if (!parsed) throw new Error("missing command");
    return [parsed];
  }

  throw new Error("unrecognized JSON shape");
}

// --- Manual form argument / env parsing -----------------------------------

export function parseArgs(text: string): string[] {
  const out: string[] = [];
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    out.push(m[1] ?? m[2] ?? m[3] ?? "");
  }
  return out;
}

export function parseEnv(text: string): [string, string][] {
  const out: [string, string][] = [];
  for (const rawLine of text.split(/\n/)) {
    const line = rawLine.trim();
    if (!line) continue;
    const eq = line.indexOf("=");
    const key = (eq < 0 ? line : line.slice(0, eq)).trim();
    const val = eq < 0 ? "" : line.slice(eq + 1).trim();
    if (!/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(key)) continue;
    out.push([key, val]);
  }
  return out;
}