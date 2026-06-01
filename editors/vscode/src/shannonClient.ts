/**
 * ShannonClient — manages the Shannon CLI subprocess with NDJSON communication.
 *
 * Spawns `shannon --prompt --output-format json-stream` and parses the
 * newline-delimited JSON output stream. Config sync is handled by passing
 * VS Code settings as environment variables (SHANNON_API_KEY, etc.).
 */

import { ChildProcess, spawn } from 'child_process';
import { EventEmitter } from 'events';
import * as vscode from 'vscode';
import { buildEnv, ShannonConfig, getConfig } from './config';

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
  start(): void {
    if (this.running) {
      return;
    }

    // Re-read config to pick up any VS Code settings changes
    this.config = getConfig();

    const cliPath = this.config.cliPath;
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
    // VS Code settings for apiKey, provider, model are passed as
    // SHANNON_API_KEY, SHANNON_PROVIDER, SHANNON_MODEL env vars,
    // which the Shannon CLI reads with priority: env vars > config files.
    const shannonEnv = buildEnv(this.config);

    this.process = spawn(cliPath, args, {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...process.env, ...shannonEnv },
    });

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
