// Shared type definitions for McpAddServerDialog sub-components.

import type { RegistryServer } from "@/lib/tauri-api";

// --- Registry package metadata (not yet on the shared TS interface) ------

export interface RegistryPackage {
  kind: string; // "npm" | "pip" | "docker" | ...
  name?: string;
  registry_url?: string;
  version?: string;
}

export type RegistryServerWithPackage = RegistryServer & {
  package?: RegistryPackage | null;
};

// --- JSON paste parsing ----------------------------------------------------

export interface ParsedMcpServer {
  name: string;
  command: string;
  args: string[];
  env: [string, string][];
}