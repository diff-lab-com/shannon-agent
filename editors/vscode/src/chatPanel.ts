/**
 * ChatPanel — WebView panel for Shannon Code conversation UI.
 *
 * Renders the chat interface and communicates with the Shannon CLI
 * via the ShannonClient. Handles all NDJSON message types including
 * text_delta, tool_use, tool_result, error, and done.
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
  private handleWebviewMessage(msg: {
    command: string;
    text?: string;
  }): void {
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

  /** Handle messages from the Shannon CLI NDJSON stream. */
  private handleShannonMessage(msg: ShannonMessage): void {
    switch (msg.type) {
      case 'text_delta':
        if (typeof msg.content === 'string') {
          this.appendAssistant(msg.content);
        }
        break;

      case 'tool_use':
        this.panel.webview.postMessage({
          command: 'toolUse',
          name: msg.name || 'unknown',
          input: msg.input
            ? this.truncate(
                typeof msg.input === 'string'
                  ? msg.input
                  : JSON.stringify(msg.input, null, 2),
                500
              )
            : '',
        });
        break;

      case 'tool_result':
        if (msg.output && typeof msg.output === 'string') {
          this.panel.webview.postMessage({
            command: 'toolResult',
            output: this.truncate(msg.output, 2000),
            isError: !!msg.is_error,
          });
        }
        break;

      case 'error':
        this.panel.webview.postMessage({
          command: 'errorMessage',
          text: `Error: ${msg.message || msg.error || 'unknown error'}`,
        });
        break;

      case 'done':
        this.appendSystem(
          msg.exit_code === 0
            ? 'Done.'
            : `Done (exit code ${msg.exit_code}).`
        );
        break;
    }
  }

  /** Truncate a string for display. */
  private truncate(text: string, maxLength: number): string {
    if (text.length <= maxLength) {
      return text;
    }
    return text.substring(0, maxLength) + '...';
  }

  /** Send a user prompt and display it. */
  private sendPrompt(text: string): void {
    this.messages.push({ role: 'user', content: text });
    this.panel.webview.postMessage({ command: 'userMessage', text });
    if (!this.client.isRunning()) {
      this.client.start();
    }
    this.client.sendPrompt(text);
  }

  /** Append assistant text (supports streaming accumulation). */
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
  <meta http-equiv="Content-Security-Policy"
    content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline' https://cdnjs.cloudflare.com; font-src https://cdnjs.cloudflare.com;">
  <title>Shannon Code</title>
  <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/styles/vs2015.min.css"
    integrity="sha384-BE+nmfgoK1j3fBxbLI64Jzf52Mx/QAyw+4O7GyPYevJnAyrljCoRtQkYNfCfuWPF" crossorigin="anonymous">
  <style>
    body { font-family: var(--vscode-font-family); margin: 0; padding: 12px; color: var(--vscode-foreground); }
    #messages { display: flex; flex-direction: column; gap: 8px; margin-bottom: 60px; }
    .msg { padding: 8px 12px; border-radius: 6px; white-space: pre-wrap; word-break: break-word; }
    .msg.user { background: var(--vscode-input-background); align-self: flex-end; max-width: 80%; }
    .msg.assistant { background: var(--vscode-editor-background); border: 1px solid var(--vscode-panel-border); }
    .msg.assistant p { margin: 0.4em 0; }
    .msg.assistant p:first-child { margin-top: 0; }
    .msg.assistant p:last-child { margin-bottom: 0; }
    .msg.assistant pre { background: var(--vscode-textCodeBlock-background, #1e1e1e); border-radius: 4px; padding: 8px 12px; overflow-x: auto; margin: 0.5em 0; }
    .msg.assistant code { font-family: var(--vscode-editor-font-family, 'Cascadia Code', 'Fira Code', monospace); font-size: 0.9em; }
    .msg.assistant code:not([class*="hljs"]) { background: var(--vscode-textCodeBlock-background, rgba(128,128,128,0.15)); padding: 1px 4px; border-radius: 3px; }
    .msg.assistant ul, .msg.assistant ol { margin: 0.4em 0; padding-left: 1.5em; }
    .msg.assistant li { margin: 0.15em 0; }
    .msg.assistant h1, .msg.assistant h2, .msg.assistant h3, .msg.assistant h4 { margin: 0.6em 0 0.3em; font-weight: 600; }
    .msg.assistant h1 { font-size: 1.3em; } .msg.assistant h2 { font-size: 1.15em; } .msg.assistant h3 { font-size: 1.05em; }
    .msg.assistant blockquote { border-left: 3px solid var(--vscode-panel-border); margin: 0.5em 0; padding-left: 0.8em; opacity: 0.85; }
    .msg.assistant a { color: var(--vscode-textLink-foreground); }
    .msg.assistant table { border-collapse: collapse; margin: 0.5em 0; }
    .msg.assistant th, .msg.assistant td { border: 1px solid var(--vscode-panel-border); padding: 4px 8px; }
    .msg.assistant th { background: var(--vscode-editor-background); font-weight: 600; }
    .msg.assistant hr { border: none; border-top: 1px solid var(--vscode-panel-border); margin: 0.8em 0; }
    .msg.system { background: var(--vscode-textBlockQuote-background); font-size: 0.9em; opacity: 0.8; }
    .msg.error { background: var(--vscode-inputValidation-errorBackground, #5a1d1d); border: 1px solid var(--vscode-inputValidation-errorBorder, #be1100); }
    .tool-block { background: var(--vscode-textBlockQuote-background); border-radius: 6px; overflow: hidden; font-size: 0.9em; margin: 2px 0; }
    .tool-header { padding: 6px 12px; cursor: pointer; display: flex; align-items: center; gap: 6px;
      background: var(--vscode-list-hoverBackground, rgba(128,128,128,0.1)); user-select: none; }
    .tool-header:hover { background: var(--vscode-list-activeSelectionBackground, rgba(128,128,128,0.2)); }
    .tool-header .arrow { font-size: 0.8em; transition: transform 0.15s; display: inline-block; }
    .tool-header .arrow.open { transform: rotate(90deg); }
    .tool-header .tool-name { font-weight: 600; }
    .tool-body { padding: 6px 12px; border-top: 1px solid var(--vscode-panel-border); }
    .tool-body.collapsed { display: none; }
    .tool-body pre { margin: 0; white-space: pre-wrap; word-break: break-all; font-family: var(--vscode-editor-font-family); font-size: 0.9em; }
    .tool-result.error-result { color: var(--vscode-errorForeground, #f48771); }
    #input-area { position: fixed; bottom: 0; left: 0; right: 0; display: flex; gap: 8px;
      padding: 12px; background: var(--vscode-sideBar-background); border-top: 1px solid var(--vscode-panel-border); }
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
  <script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/highlight.min.js"
    integrity="sha384-F/bZzf7p3Joyp5psL90p/p89AZJsndkSoGwRpXcZhleCWhd8SnRuoYo4d0yirjJp" crossorigin="anonymous"></script>
  <script src="https://cdnjs.cloudflare.com/ajax/libs/markdown-it/14.0.0/markdown-it.min.js"
    integrity="sha384-ZSs6LKr2GoUPDyHrN+rCQgyHL1yUyok5xMniSrgeRG7rUvA6vTmxronM1eZOfjgz" crossorigin="anonymous"></script>
  <script>
    const vscode = acquireVsCodeApi();
    const messages = document.getElementById('messages');
    const input = document.getElementById('prompt-input');
    const md = window.markdownit({ html: false, linkify: true, breaks: true,
      highlight: function(str, lang) {
        if (lang && hljs.getLanguage(lang)) { try { return hljs.highlight(str, { language: lang }).value; } catch(e) {} }
        try { return hljs.highlightAuto(str).value; } catch(e) {}
        return '';
      }
    });

    function scrollToBottom() { messages.scrollTop = messages.scrollHeight; }

    function renderMarkdown(text) { return md.render(text); }

    function addMessage(role, text) {
      const div = document.createElement('div');
      div.className = 'msg ' + role;
      if (role === 'assistant') { div.innerHTML = renderMarkdown(text); }
      else { div.textContent = text; }
      messages.appendChild(div);
      scrollToBottom();
    }

    function appendToLast(role, text) {
      const msgs = messages.querySelectorAll('.msg.' + role);
      if (msgs.length > 0) {
        const last = msgs[msgs.length - 1];
        const raw = (last.dataset.raw || '') + text;
        last.dataset.raw = raw;
        last.innerHTML = renderMarkdown(raw);
        scrollToBottom();
      }
    }

    let toolCounter = 0;

    function addToolBlock(name, inputText) {
      const id = 'tool-' + (++toolCounter);
      const block = document.createElement('div');
      block.className = 'tool-block';
      block.id = id;

      const header = document.createElement('div');
      header.className = 'tool-header';
      const arrow = document.createElement('span');
      arrow.className = 'arrow';
      arrow.textContent = '\u25B6';
      const nameSpan = document.createElement('span');
      nameSpan.className = 'tool-name';
      nameSpan.textContent = name;
      header.appendChild(arrow);
      header.appendChild(nameSpan);

      const body = document.createElement('div');
      body.className = 'tool-body collapsed';
      body.id = id + '-body';

      if (inputText) {
        const pre = document.createElement('pre');
        pre.textContent = inputText;
        body.appendChild(pre);
      }

      header.addEventListener('click', () => {
        const isCollapsed = body.classList.contains('collapsed');
        if (isCollapsed) { body.classList.remove('collapsed'); arrow.classList.add('open'); }
        else { body.classList.add('collapsed'); arrow.classList.remove('open'); }
      });

      block.appendChild(header);
      block.appendChild(body);
      messages.appendChild(block);
      scrollToBottom();
      return id;
    }

    function appendToolResult(toolId, output, isError) {
      const block = document.getElementById(toolId);
      if (!block) { return; }
      const body = block.querySelector('.tool-body');
      if (!body) { return; }

      const resultDiv = document.createElement('div');
      resultDiv.className = 'tool-result' + (isError ? ' error-result' : '');
      const pre = document.createElement('pre');
      pre.textContent = (isError ? '[Error] ' : '') + output;
      resultDiv.appendChild(pre);
      body.appendChild(resultDiv);

      if (isError) {
        body.classList.remove('collapsed');
        const arrowEl = block.querySelector('.arrow');
        if (arrowEl) { arrowEl.classList.add('open'); }
      }
      scrollToBottom();
    }

    let lastToolId = null;

    window.addEventListener('message', (event) => {
      const msg = event.data;
      switch (msg.command) {
        case 'userMessage': addMessage('user', msg.text); break;
        case 'assistantMessage':
          const div = document.createElement('div');
          div.className = 'msg assistant';
          div.dataset.raw = msg.text;
          div.innerHTML = renderMarkdown(msg.text);
          messages.appendChild(div);
          scrollToBottom();
          break;
        case 'appendAssistant': appendToLast('assistant', msg.text); break;
        case 'systemMessage': addMessage('system', msg.text); break;
        case 'errorMessage': addMessage('error', msg.text); break;
        case 'toolUse': lastToolId = addToolBlock(msg.name, msg.input); break;
        case 'toolResult':
          if (lastToolId) { appendToolResult(lastToolId, msg.output, msg.isError); lastToolId = null; }
          else { addMessage('system', (msg.isError ? '[Error] ' : '') + msg.output); }
          break;
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
