/**
 * Configuration sync between VS Code settings and Shannon CLI.
 */

import * as vscode from 'vscode';

export interface ShannonConfig {
  cliPath: string;
  provider: string;
  model: string;
  apiKey: string;
}

/** Read Shannon configuration from VS Code settings. */
export function getConfig(): ShannonConfig {
  const config = vscode.workspace.getConfiguration('shannon');
  return {
    cliPath: config.get<string>('cliPath', 'shannon'),
    provider: config.get<string>('provider', 'anthropic'),
    model: config.get<string>('model', ''),
    apiKey: config.get<string>('apiKey', ''),
  };
}

/** Build environment variables for the Shannon CLI subprocess. */
export function buildEnv(config: ShannonConfig): Record<string, string> {
  const env: Record<string, string> = {};
  if (config.apiKey) {
    env.SHANNON_API_KEY = config.apiKey;
  }
  if (config.provider) {
    env.SHANNON_PROVIDER = config.provider;
  }
  if (config.model) {
    env.SHANNON_MODEL = config.model;
  }
  return env;
}
