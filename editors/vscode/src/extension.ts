/**
 * Shannon Code — VS Code extension entry point.
 *
 * Provides a Chat WebView that communicates with the Shannon CLI
 * via subprocess with NDJSON streaming.
 */

import * as vscode from 'vscode';
import { ShannonClient } from './shannonClient';
import { ChatPanel } from './chatPanel';
import { getConfig } from './config';

let client: ShannonClient;

export function activate(context: vscode.ExtensionContext): void {
  const config = getConfig();
  client = new ShannonClient(
    vscode.workspace.getConfiguration('shannon')
  );

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
        client.start();
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

  context.subscriptions.push(openChat, sendPrompt, stopGeneration, client);
}

export function deactivate(): void {
  client?.dispose();
}
