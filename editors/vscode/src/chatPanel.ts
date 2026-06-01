/**
 * ChatPanel — WebView panel for Shannon Code conversation UI.
 */

import * as vscode from 'vscode';
import { ShannonClient, ShannonMessage } from './shannonClient';

interface ChatMessage {
  role: 'user' | 'assistant' | 'system';
  content: string;
}

export class ChatPanel {
  public static currentPanel: ChatPanel | undefined;
  private readonly panel: vscode.WebviewPanel;
  private messages: ChatMessage[] = [];
  private client: ShannonClient;

  private constructor(panel: vscode.WebviewPanel, client: ShannonClient) {
    this.panel = panel;
    this.client = client;

    this.panel.webview.html = this.getHtml();

    this.panel.webview.onDidReceiveMessage((msg) => {
      this.handleWebviewMessage(msg);
    });

    this.panel.onDidDispose(() => {
      this.dispose();
    });

    // Listen for Shannon CLI messages
    this.client.on('message', (msg: ShannonMessage) => {
      this.handleShannonMessage(msg);
    });

    this.client.on('text', (text: string) => {
      this.appendAssistant(text);
    });
  }

  /** Show or create the chat panel. */
  public static show(client: ShannonClient): void {
    const column = vscode.window.activeTextEditor
      ? vscode.window.activeTextEditor.viewColumn
      : undefined;

    if (ChatPanel.currentPanel) {
      ChatPanel.currentPanel.panel.reveal(column);
      return;
    }

    const panel = vscode.window.createWebviewPanel(
      'shannonChat',
      'Shannon Code',
      column || vscode.ViewColumn.One,
      {
        enableScripts: true,
        retainContextWhenHidden: true,
      }
    );

    ChatPanel.currentPanel = new ChatPanel(panel, client);
  }

  /** Handle messages from the WebView. */
  private handleWebviewMessage(msg: { command: string; text?: string }): void {
    switch (msg.command) {
      case 'sendPrompt':
        if (msg.text) {
          this.sendPrompt(msg.text);
        }
        break;
      case 'stop':
        this.client.stop();
        break;
    }
  }

  /** Handle messages from the Shannon CLI. */
  private handleShannonMessage(msg: ShannonMessage): void {
    switch (msg.type) {
      case 'assistant':
        if (typeof msg.content === 'string') {
          this.appendAssistant(msg.content);
        }
        break;
      case 'tool_use':
        this.appendSystem(
          `Tool: ${msg.tool_name || 'unknown'}`
        );
        break;
      case 'tool_result':
        if (msg.content && typeof msg.content === 'string') {
          this.appendSystem(`Result: ${msg.content.substring(0, 200)}`);
        }
        break;
      case 'error':
        this.appendSystem(`Error: ${msg.error || 'unknown error'}`);
        break;
    }
  }

  /** Send a user prompt and display it. */
  private sendPrompt(text: string): void {
    this.messages.push({ role: 'user', content: text });
    this.panel.webview.postMessage({ command: 'userMessage', text });
    this.client.sendPrompt(text);
  }

  /** Append assistant text. */
  private appendAssistant(text: string): void {
    const last = this.messages[this.messages.length - 1];
    if (last?.role === 'assistant') {
      last.content += text;
      this.panel.webview.postMessage({ command: 'appendAssistant', text });
    } else {
      this.messages.push({ role: 'assistant', content: text });
      this.panel.webview.postMessage({ command: 'assistantMessage', text });
    }
  }

  /** Append a system message. */
  private appendSystem(text: string): void {
    this.messages.push({ role: 'system', content: text });
    this.panel.webview.postMessage({ command: 'systemMessage', text });
  }

  /** Get the WebView HTML. */
  private getHtml(): string {
    return /* html */ `
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <link rel="stylesheet" href="${this.panel.webview.asWebviewUri(
    vscode.Uri.file('')
  )}media/chat.css">
  <title>Shannon Code</title>
  <style>
    body { font-family: var(--vscode-font-family); margin: 0; padding: 12px; color: var(--vscode-foreground); }
    #messages { display: flex; flex-direction: column; gap: 8px; margin-bottom: 12px; }
    .msg { padding: 8px 12px; border-radius: 6px; white-space: pre-wrap; word-break: break-word; }
    .msg.user { background: var(--vscode-input-background); align-self: flex-end; max-width: 80%; }
    .msg.assistant { background: var(--vscode-editor-background); border: 1px solid var(--vscode-panel-border); }
    .msg.system { background: var(--vscode-textBlockQuote-background); font-size: 0.9em; opacity: 0.8; }
    #input-area { display: flex; gap: 8px; }
    #prompt-input { flex: 1; padding: 8px; border: 1px solid var(--vscode-input-border);
      background: var(--vscode-input-background); color: var(--vscode-input-foreground);
      border-radius: 4px; font-family: var(--vscode-editor-font-family); resize: vertical; min-height: 40px; }
    button { padding: 8px 16px; border: none; border-radius: 4px; cursor: pointer;
      background: var(--vscode-button-background); color: var(--vscode-button-foreground); }
    button:hover { background: var(--vscode-button-hoverBackground); }
    button.secondary { background: var(--vscode-button-secondaryBackground);
      color: var(--vscode-button-secondaryForeground); }
    button.secondary:hover { background: var(--vscode-button-secondaryHoverBackground); }
  </style>
</head>
<body>
  <div id="messages"></div>
  <div id="input-area">
    <textarea id="prompt-input" placeholder="Ask Shannon..." rows="2"></textarea>
    <button id="send-btn">Send</button>
    <button id="stop-btn" class="secondary">Stop</button>
  </div>
  <script>
    const vscode = acquireVsCodeApi();
    const messages = document.getElementById('messages');
    const input = document.getElementById('prompt-input');

    function addMessage(role, text) {
      const div = document.createElement('div');
      div.className = 'msg ' + role;
      div.textContent = text;
      messages.appendChild(div);
      messages.scrollTop = messages.scrollHeight;
    }

    function appendToLast(role, text) {
      const msgs = messages.querySelectorAll('.msg.' + role);
      if (msgs.length > 0) {
        const last = msgs[msgs.length - 1];
        last.textContent += text;
        messages.scrollTop = messages.scrollHeight;
      }
    }

    window.addEventListener('message', (event) => {
      const msg = event.data;
      switch (msg.command) {
        case 'userMessage': addMessage('user', msg.text); break;
        case 'assistantMessage': addMessage('assistant', msg.text); break;
        case 'appendAssistant': appendToLast('assistant', msg.text); break;
        case 'systemMessage': addMessage('system', msg.text); break;
      }
    });

    document.getElementById('send-btn').addEventListener('click', () => {
      const text = input.value.trim();
      if (text) { vscode.postMessage({ command: 'sendPrompt', text }); input.value = ''; }
    });

    document.getElementById('stop-btn').addEventListener('click', () => {
      vscode.postMessage({ command: 'stop' });
    });

    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        document.getElementById('send-btn').click();
      }
    });
  </script>
</body>
</html>`;
  }

  /** Dispose the panel. */
  public dispose(): void {
    ChatPanel.currentPanel = undefined;
    this.panel.dispose();
  }
}
