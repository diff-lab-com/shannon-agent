/**
 * ShannonClient — manages the Shannon CLI subprocess with NDJSON communication.
 *
 * Spawns `shannon --prompt --output-format json-stream` and parses the
 * newline-delimited JSON output stream. Config sync is handled by passing
 * VS Code settings as environment variables (SHANNON_API_KEY, etc.).
 */

import { ChildProcess, spawn } from 'child_process';
import { EventEmitter } from 'events';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { buildEnv, ShannonConfig, getConfig } from './config';

/**
 * Search for the Shannon CLI binary.
 *
 * Checks in order:
 * 1. User-configured `shannon.cliPath` (absolute or relative)
 * 2. `shannon` in PATH (via `which`/`where`)
 * 3. Common install locations
 *
 * Returns the resolved path or null if not found.
 */
export async function findShannonBinary(configuredPath: string): Promise<string | null> {
  // 1. Try the configured path
  if (configuredPath && configuredPath !== 'shannon') {
    if (fs.existsSync(configuredPath)) {
      return configuredPath;
    }
    // Try resolving relative to workspace
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (workspaceFolders) {
      const resolved = path.join(workspaceFolders[0].uri.fsPath, configuredPath);
      if (fs.existsSync(resolved)) {
        return resolved;
      }
    }
  }

  // 2. Try `shannon` in PATH
  const pathResult = await checkPath('shannon');
  if (pathResult) {
    return pathResult;
  }

  // 3. Common install locations
  const commonPaths = [
    path.join(process.env.HOME || '', '.cargo', 'bin', 'shannon'),
    '/usr/local/bin/shannon',
    '/usr/bin/shannon',
  ];
  for (const p of commonPaths) {
    if (fs.existsSync(p)) {
      return p;
    }
  }

  return null;
}

/** Check if a binary is available in PATH. */
function checkPath(binary: string): Promise<string | null> {
  const cmd = process.platform === 'win32' ? 'where' : 'which';
  return new Promise((resolve) => {
    const child = spawn(cmd, [binary], { stdio: 'pipe' });
    let output = '';
    child.stdout?.on('data', (data: Buffer) => {
      output += data.toString();
    });
    child.on('close', (code) => {
      if (code === 0 && output.trim()) {
        resolve(output.trim().split('\n')[0]);
      } else {
        resolve(null);
      }
    });
    child.on('error', () => resolve(null));
  });
}

/** Show an error message when the Shannon CLI binary cannot be found. */
export function showBinaryNotFound(): void {
  vscode.window.showErrorMessage(
    'Shannon CLI not found. Install with `cargo install --git https://github.com/shannon-agent/shannon-agent.git shannon-cli` or set the path in Settings.',
    'Open Settings'
  ).then((action) => {
    if (action === 'Open Settings') {
      vscode.commands.executeCommand('workbench.action.openSettings', 'shannon.cliPath');
    }
  });
}

/** Typed Shannon NDJSON message — matches the OutputEvent enum from shannon-cli. */
export interface ShannonMessage {
  type: string;
  [key: string]: unknown;
}

/** Specific message types for convenience. */
export interface TextDeltaMessage extends ShannonMessage {
  type: 'text_delta';
  content: string;
}

export interface ToolUseMessage extends ShannonMessage {
  type: 'tool_use';
  name: string;
  input: unknown;
}

export interface ToolResultMessage extends ShannonMessage {
  type: 'tool_result';
  name: string;
  output: string;
  is_error: boolean;
}

export interface ErrorMessage extends ShannonMessage {
  type: 'error';
  message: string;
}

export interface DoneMessage extends ShannonMessage {
  type: 'done';
  exit_code: number;
}

/** Type guard for text_delta messages. */
export function isTextDelta(msg: ShannonMessage): msg is TextDeltaMessage {
  return msg.type === 'text_delta';
}

/** Type guard for tool_use messages. */
export function isToolUse(msg: ShannonMessage): msg is ToolUseMessage {
  return msg.type === 'tool_use';
}

/** Type guard for tool_result messages. */
export function isToolResult(msg: ShannonMessage): msg is ToolResultMessage {
  return msg.type === 'tool_result';
}

/** Type guard for error messages. */
export function isError(msg: ShannonMessage): msg is ErrorMessage {
  return msg.type === 'error';
}

/** Type guard for done messages. */
export function isDone(msg: ShannonMessage): msg is DoneMessage {
  return msg.type === 'done';
}

export class ShannonClient extends EventEmitter {
  private process: ChildProcess | null = null;
  private buffer = '';
  private running = false;
  private config: ShannonConfig;

  constructor(private vscodeConfig: vscode.WorkspaceConfiguration) {
    super();
    this.config = getConfig();
  }

  /** Start the Shannon CLI subprocess. */
  async start(): Promise<void> {
    if (this.running) {
      return;
    }

    // Re-read config to pick up any VS Code settings changes
    this.config = getConfig();

    const cliPath = await findShannonBinary(this.config.cliPath);
    if (!cliPath) {
      showBinaryNotFound();
      return;
    }

    const provider = this.config.provider;
    const model = this.config.model;

    const args = ['--prompt', '--output-format', 'json-stream'];
    if (provider) {
      args.push('--provider', provider);
    }
    if (model) {
      args.push('--model', model);
    }

    // Build env from VS Code settings — this is the config sync mechanism.
    const shannonEnv = buildEnv(this.config);

    try {
      this.process = spawn(cliPath, args, {
        stdio: ['pipe', 'pipe', 'pipe'],
        env: { ...process.env, ...shannonEnv },
      });
    } catch (err) {
      vscode.window.showErrorMessage(
        `Shannon: Failed to start CLI: ${err instanceof Error ? err.message : String(err)}`
      );
      return;
    }

    this.running = true;

    this.process.stdout?.on('data', (data: Buffer) => {
      this.handleData(data.toString());
    });

    this.process.stderr?.on('data', (data: Buffer) => {
      this.emit('stderr', data.toString());
    });

    this.process.on('close', (code) => {
      this.running = false;
      this.process = null;
      this.emit('close', code);
    });

    this.process.on('error', (err) => {
      this.running = false;
      this.process = null;
      this.emit('error', err);
      if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
        showBinaryNotFound();
      } else if ((err as NodeJS.ErrnoException).code === 'EACCES') {
        vscode.window.showErrorMessage(
          `Shannon: Permission denied executing ${cliPath}. Check file permissions.`
        );
      } else {
        vscode.window.showErrorMessage(
          `Shannon: CLI error: ${err.message}`
        );
      }
    });
  }

  /** Send a prompt to the CLI stdin. */
  sendPrompt(prompt: string): void {
    if (!this.process?.stdin) {
      this.start();
    }
    this.process?.stdin?.write(prompt + '\n');
  }

  /** Stop the CLI subprocess. */
  stop(): void {
    if (this.process) {
      this.process.kill('SIGTERM');
      this.process = null;
      this.running = false;
    }
  }

  /** Check if the client is running. */
  isRunning(): boolean {
    return this.running;
  }

  /** Get the current config. */
  getConfig(): ShannonConfig {
    return this.config;
  }

  /** Parse NDJSON from the stdout stream. */
  private handleData(data: string): void {
    this.buffer += data;
    const lines = this.buffer.split('\n');
    this.buffer = lines.pop() || '';

    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed) {
        continue;
      }
      try {
        const msg: ShannonMessage = JSON.parse(trimmed);
        this.emit('message', msg);

        // Emit typed events for convenience
        if (msg.type) {
          this.emit(msg.type, msg);
        }
      } catch {
        // Non-JSON line — emit as raw text
        this.emit('text', trimmed);
      }
    }
  }

  /** Dispose: stop the process and remove all listeners. */
  dispose(): void {
    this.stop();
    this.removeAllListeners();
  }
}
