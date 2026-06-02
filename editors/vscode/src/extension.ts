/**
 * Shannon Code — VS Code extension entry point.
 *
 * Provides:
 * - Chat WebView communicating with Shannon CLI via NDJSON
 * - Diff viewer for reviewing file changes (accept/reject)
 * - File apply for writing accepted changes to disk
 * - Config sync between VS Code settings and Shannon CLI env vars
 */

import * as vscode from 'vscode';
import { ShannonClient } from './shannonClient';
import { ChatPanel } from './chatPanel';
import { getConfig } from './config';
import { DiffViewer, DiffContentProvider } from './diffViewer';

let client: ShannonClient;
let diffViewer: DiffViewer;

export function activate(context: vscode.ExtensionContext): void {
  const config = getConfig();
  client = new ShannonClient(vscode.workspace.getConfiguration('shannon'));

  // Output channel for extension logging
  const outputChannel = vscode.window.createOutputChannel('Shannon Code');
  context.subscriptions.push(outputChannel);

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

  // --- Commands ---

  const openChat = vscode.commands.registerCommand('shannon.openChat', () => {
    ChatPanel.show(client);
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
        await client.start();
        client.sendPrompt(prompt);
      }
    }
  );

  const stopGeneration = vscode.commands.registerCommand(
    'shannon.stopGeneration',
    () => {
      client.stop();
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
}

export function deactivate(): void {
  client?.dispose();
  diffViewer?.dispose();
}
