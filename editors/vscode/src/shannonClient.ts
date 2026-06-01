/**
 * ShannonClient — manages the Shannon CLI subprocess with NDJSON communication.
 *
 * Spawns `shannon --prompt --output-format json-stream` and parses the
 * newline-delimited JSON output stream.
 */

import { ChildProcess, spawn } from 'child_process';
import { EventEmitter } from 'events';
import * as vscode from 'vscode';

export interface ShannonMessage {
  type: string;
  [key: string]: unknown;
}

export class ShannonClient extends EventEmitter {
  private process: ChildProcess | null = null;
  private buffer = '';
  private running = false;

  constructor(private config: vscode.WorkspaceConfiguration) {
    super();
  }

  /** Start the Shannon CLI subprocess. */
  start(): void {
    if (this.running) {
      return;
    }

    const cliPath = this.config.get<string>('cliPath', 'shannon');
    const provider = this.config.get<string>('provider', 'anthropic');
    const model = this.config.get<string>('model', '');

    const args = ['--prompt', '--output-format', 'json-stream'];
    if (provider) {
      args.push('--provider', provider);
    }
    if (model) {
      args.push('--model', model);
    }

    this.process = spawn(cliPath, args, {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...process.env },
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
