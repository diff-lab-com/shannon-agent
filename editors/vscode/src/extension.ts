/**
 * Shannon Code — VS Code extension entry point.
 *
 * Provides:
 * - Chat WebView communicating with Shannon CLI via NDJSON
 * - Diff viewer for reviewing file changes (accept/reject)
 * - File apply for writing accepted changes to disk
 * - Config sync between VS Code settings and Shannon CLI env vars
 * - Status bar indicator for connection state
 */

import * as vscode from 'vscode';
import { ShannonClient } from './shannonClient';
import { ChatPanel } from './chatPanel';
import { getConfig } from './config';
import { DiffViewer, DiffContentProvider } from './diffViewer';

let client: ShannonClient;
let diffViewer: DiffViewer;
let statusBarItem: vscode.StatusBarItem;

export function activate(context: vscode.ExtensionContext): void {
  const config = getConfig();
  client = new ShannonClient(vscode.workspace.getConfiguration('shannon'));

  // Output channel for extension logging
  const outputChannel = vscode.window.createOutputChannel('Shannon Code');
  context.subscriptions.push(outputChannel);

  // Status bar indicator
  statusBarItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    100
  );
  statusBarItem.command = 'shannon.openChat';
  updateStatusBar('disconnected');
  statusBarItem.show();
  context.subscriptions.push(statusBarItem);

  // Track client lifecycle for status bar
  client.on('close', () => updateStatusBar('disconnected'));
  client.on('error', () => updateStatusBar('error'));

  // Diff viewer for reviewing file changes
  diffViewer = new DiffViewer(outputChannel);
  context.subscriptions.push(diffViewer);

  // Register content providers for virtual diff documents
  const originalProvider = new DiffContentProvider(diffViewer);
  const proposedProvider = new DiffContentProvider(diffViewer);

  context.subscriptions.push(
    vscode.workspace.registerTextDocumentContentProvider(
      'shannon-diff-original',
      originalProvider
    ),
    vscode.workspace.registerTextDocumentContentProvider(
      'shannon-diff-proposed',
      proposedProvider
    )
  );

  // Wire diff viewer to Shannon CLI messages
  client.on('message', (msg) => {
    const change = diffViewer.handleMessage(msg);
    if (change) {
      // Show diff for file-editing tools
      diffViewer.showDiff(change);
    }
  });

  // Track working state from messages
  client.on('message', (msg) => {
    const msgType = (msg as { type?: string }).type;
    if (msgType === 'text_delta' || msgType === 'tool_use') {
      updateStatusBar('working');
    } else if (msgType === 'done') {
      updateStatusBar('connected');
    }
  });

  // --- Commands ---

  const openChat = vscode.commands.registerCommand('shannon.openChat', () => {
    ChatPanel.show(client);
    updateStatusBar('connected');
  });

  const sendPrompt = vscode.commands.registerCommand(
    'shannon.sendPrompt',
    async () => {
      const prompt = await vscode.window.showInputBox({
        prompt: 'Enter prompt for Shannon',
        placeHolder: 'What would you like to do?',
      });
      if (prompt) {
        ChatPanel.show(client);
        updateStatusBar('working');
        await client.start();
        client.sendPrompt(prompt);
      }
    }
  );

  const stopGeneration = vscode.commands.registerCommand(
    'shannon.stopGeneration',
    () => {
      client.stop();
      updateStatusBar('connected');
      vscode.window.showInformationMessage('Shannon: Generation stopped.');
    }
  );

  const showPendingChanges = vscode.commands.registerCommand(
    'shannon.showPendingChanges',
    async () => {
      const pending = diffViewer.getPendingChanges();
      if (pending.length === 0) {
        vscode.window.showInformationMessage(
          'Shannon: No pending file changes to review.'
        );
        return;
      }

      // Show a quick pick with the pending changes
      const items = pending.map((c) => ({
        label: vscode.workspace.asRelativePath(c.filePath),
        description: c.toolName,
        detail: c.resolved
          ? 'Resolved'
          : 'Pending review',
        change: c,
      }));

      const selected = await vscode.window.showQuickPick(items, {
        placeHolder: 'Select a change to review',
        canPickMany: false,
      });

      if (selected) {
        await diffViewer.showDiff(selected.change);
      }
    }
  );

  const acceptAllChanges = vscode.commands.registerCommand(
    'shannon.acceptAllChanges',
    async () => {
      const pending = diffViewer.getPendingChanges();
      if (pending.length === 0) {
        vscode.window.showInformationMessage(
          'Shannon: No pending file changes.'
        );
        return;
      }

      const action = await vscode.window.showWarningMessage(
        `Accept all ${pending.length} pending change(s)?`,
        { modal: true },
        'Accept All'
      );

      if (action === 'Accept All') {
        await diffViewer.acceptAll();
        vscode.window.showInformationMessage(
          `Shannon: Accepted ${pending.length} change(s).`
        );
      }
    }
  );

  const rejectAllChanges = vscode.commands.registerCommand(
    'shannon.rejectAllChanges',
    async () => {
      const pending = diffViewer.getPendingChanges();
      if (pending.length === 0) {
        vscode.window.showInformationMessage(
          'Shannon: No pending file changes.'
        );
        return;
      }

      const action = await vscode.window.showWarningMessage(
        `Reject all ${pending.length} pending change(s)?`,
        { modal: true },
        'Reject All'
      );

      if (action === 'Reject All') {
        await diffViewer.rejectAll();
        vscode.window.showInformationMessage(
          `Shannon: Rejected ${pending.length} change(s).`
        );
      }
    }
  );

  const openSettings = vscode.commands.registerCommand(
    'shannon.openSettings',
    () => {
      vscode.commands.executeCommand(
        'workbench.action.openSettings',
        'shannon'
      );
    }
  );

  context.subscriptions.push(
    openChat,
    sendPrompt,
    stopGeneration,
    showPendingChanges,
    acceptAllChanges,
    rejectAllChanges,
    openSettings,
    client
  );

  // Welcome message on first activation
  const shownKey = 'shannon.welcomeShown';
  const welcomeShown = context.globalState.get<boolean>(shownKey, false);
  if (!welcomeShown) {
    vscode.window.showInformationMessage(
      'Shannon Code is ready. Open the chat with the status bar icon or run "Shannon: Open Chat" from the command palette.',
      'Open Chat',
      'Dismiss'
    ).then((action) => {
      if (action === 'Open Chat') {
        vscode.commands.executeCommand('shannon.openChat');
      }
    });
    context.globalState.update(shownKey, true);
  }
}

function updateStatusBar(state: 'disconnected' | 'connected' | 'working' | 'error'): void {
  switch (state) {
    case 'disconnected':
      statusBarItem.text = '$(plug) Shannon';
      statusBarItem.tooltip = 'Shannon Code — Click to open chat';
      statusBarItem.backgroundColor = undefined;
      break;
    case 'connected':
      statusBarItem.text = '$(check) Shannon';
      statusBarItem.tooltip = 'Shannon Code — Connected';
      statusBarItem.backgroundColor = undefined;
      break;
    case 'working':
      statusBarItem.text = '$(loading~spin) Shannon';
      statusBarItem.tooltip = 'Shannon Code — Working...';
      statusBarItem.backgroundColor = undefined;
      break;
    case 'error':
      statusBarItem.text = '$(error) Shannon';
      statusBarItem.tooltip = 'Shannon Code — Error (click to retry)';
      statusBarItem.backgroundColor = new vscode.ThemeColor(
        'statusBarItem.errorBackground'
      );
      break;
  }
}

export function deactivate(): void {
  client?.dispose();
  diffViewer?.dispose();
}
