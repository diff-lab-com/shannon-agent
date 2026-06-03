/**
 * DiffViewer — Shows file changes from Shannon CLI with accept/reject actions.
 *
 * Intercepts tool_use events for Edit/Write tools and presents a VS Code diff
 * editor so the user can review and accept or reject changes.
 */

import * as vscode from 'vscode';
import * as path from 'path';
import { ShannonMessage } from './shannonClient';

/** Represents a pending file change awaiting user review. */
export interface PendingChange {
  /** Tool that produced this change (Edit or Write). */
  toolName: string;
  /** Absolute workspace path of the target file. */
  filePath: string;
  /** Original content before the change (empty string for new files). */
  originalContent: string;
  /** Proposed new content. */
  proposedContent: string;
  /** Resolves to true (accept) or false (reject). */
  resolution: Promise<boolean>;
  /** Internal resolve function bound to the resolution promise. */
  resolve: (accepted: boolean) => void;
  /** Whether the change has been resolved. */
  resolved: boolean;
}

/** Manages pending file changes and presents diff editors for review. */
export class DiffViewer implements vscode.Disposable {
  private pendingChanges: PendingChange[] = [];
  private disposables: vscode.Disposable[] = [];
  private outputChannel: vscode.OutputChannel;

  constructor(outputChannel: vscode.OutputChannel) {
    this.outputChannel = outputChannel;
  }

  /**
   * Handle a Shannon NDJSON message. Returns a PendingChange if the message
   * represents a file-editing tool use that should be reviewed, or undefined
   * otherwise.
   */
  public handleMessage(msg: ShannonMessage): PendingChange | undefined {
    if (msg.type !== 'tool_use') {
      return undefined;
    }

    const toolName = msg.name as string | undefined;
    if (toolName !== 'Edit' && toolName !== 'Write') {
      return undefined;
    }

    const input = msg.input as Record<string, unknown> | undefined;
    if (!input || typeof input !== 'object') {
      return undefined;
    }

    const filePath = input.file_path as string
      ?? input.path as string;
    if (!filePath || typeof filePath !== 'string') {
      return undefined;
    }

    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (!workspaceFolders || workspaceFolders.length === 0) {
      return undefined;
    }

    const workspaceRoot = workspaceFolders[0].uri.fsPath;
    const absolutePath = path.isAbsolute(filePath)
      ? filePath
      : path.join(workspaceRoot, filePath);

    let originalContent = '';
    let proposedContent = '';
    let editMeta: { oldString: string; newString: string } | undefined;

    if (toolName === 'Write') {
      // Write tool replaces entire file content
      originalContent = '';
      proposedContent = (input.content as string) ?? '';
    } else if (toolName === 'Edit') {
      // Edit tool replaces old_string with new_string
      const oldString = (input.old_string as string) ?? '';
      const newString = (input.new_string as string) ?? '';
      // We don't have the full file content here, so store the diff parts
      // The actual diff will be computed when we read the current file
      originalContent = oldString;
      proposedContent = newString;
      editMeta = { oldString, newString };
    }

    const change = this.createPendingChange(
      toolName,
      absolutePath,
      originalContent,
      proposedContent,
      editMeta
    );

    this.pendingChanges.push(change);
    return change;
  }

  /** Show a VS Code diff editor for the given pending change. */
  public async showDiff(change: PendingChange): Promise<void> {
    const fileName = path.basename(change.filePath);
    const title = `${fileName} — Shannon Change (Accept / Reject)`;

    // Read the current file content for the "original" side
    let currentContent = '';
    try {
      const doc = await vscode.workspace.openTextDocument(change.filePath);
      currentContent = doc.getText();
    } catch {
      // File may not exist yet (new file via Write tool)
    }

    // Compute the proposed content
    let proposedContent: string;
    if (change.toolName === 'Edit') {
      // Apply the edit replacement to current content
      const editMeta = (change as PendingChange & { editMeta?: { oldString: string; newString: string } }).editMeta;
      if (editMeta) {
        proposedContent = currentContent.replace(editMeta.oldString, editMeta.newString);
      } else {
        proposedContent = change.proposedContent;
      }
    } else {
      // Write tool — full replacement
      proposedContent = change.proposedContent;
    }

    // Create virtual documents for the diff editor
    const originalUri = vscode.Uri.parse(`shannon-diff-original://${change.filePath}`).with({
      query: encodeURIComponent(currentContent),
    });
    const proposedUri = vscode.Uri.parse(`shannon-diff-proposed://${change.filePath}`).with({
      query: encodeURIComponent(proposedContent),
    });

    // Store the content so the TextDocumentContentProvider can serve it
    this.setContent(originalUri, currentContent);
    this.setContent(proposedUri, proposedContent);

    await vscode.commands.executeCommand(
      'vscode.diff',
      originalUri,
      proposedUri,
      title,
      { preserveFocus: false }
    );

    // Show accept/reject notification
    const action = await vscode.window.showInformationMessage(
      `Apply Shannon's change to ${fileName}?`,
      { modal: false },
      'Accept',
      'Reject'
    );

    if (action === 'Accept') {
      change.resolve(true);
      change.resolved = true;
      this.outputChannel.appendLine(`Accepted change: ${change.filePath}`);
      this.removeFromPending(change);
    } else {
      change.resolve(false);
      change.resolved = true;
      this.outputChannel.appendLine(`Rejected change: ${change.filePath}`);
      this.removeFromPending(change);
    }
  }

  /** Get all unresolved pending changes. */
  public getPendingChanges(): readonly PendingChange[] {
    return this.pendingChanges.filter(c => !c.resolved);
  }

  /** Accept all pending changes without showing diffs. */
  public async acceptAll(): Promise<void> {
    for (const change of this.pendingChanges) {
      if (!change.resolved) {
        change.resolve(true);
        change.resolved = true;
      }
    }
    this.pendingChanges = [];
  }

  /** Reject all pending changes without showing diffs. */
  public async rejectAll(): Promise<void> {
    for (const change of this.pendingChanges) {
      if (!change.resolved) {
        change.resolve(false);
        change.resolved = true;
      }
    }
    this.pendingChanges = [];
  }

  /** Create a PendingChange with a wired-up resolution promise. */
  private createPendingChange(
    toolName: string,
    filePath: string,
    originalContent: string,
    proposedContent: string,
    editMeta?: { oldString: string; newString: string }
  ): PendingChange {
    let resolveFunc: (accepted: boolean) => void;
    const resolution = new Promise<boolean>((resolve) => {
      resolveFunc = resolve;
    });

    const change: PendingChange = {
      toolName,
      filePath,
      originalContent,
      proposedContent,
      resolution,
      resolve: resolveFunc!,
      resolved: false,
    };

    // Attach edit metadata if this is an Edit tool change
    if (editMeta) {
      (change as PendingChange & { editMeta?: { oldString: string; newString: string } }).editMeta = editMeta;
    }

    return change;
  }

  private contentStore = new Map<string, string>();

  private setContent(uri: vscode.Uri, content: string): void {
    this.contentStore.set(uri.toString(), content);
  }

  /** Get stored content for a URI (used by the content provider). */
  public getContent(uri: vscode.Uri): string | undefined {
    return this.contentStore.get(uri.toString());
  }

  private removeFromPending(change: PendingChange): void {
    const idx = this.pendingChanges.indexOf(change);
    if (idx >= 0) {
      this.pendingChanges.splice(idx, 1);
    }
  }

  public dispose(): void {
    for (const d of this.disposables) {
      d.dispose();
    }
    this.pendingChanges = [];
    this.contentStore.clear();
  }
}

/**
 * TextDocumentContentProvider that serves virtual documents for the diff
 * editor. Registered for shannon-diff-original:// and shannon-diff-proposed://
 * schemes.
 */
export class DiffContentProvider implements vscode.TextDocumentContentProvider {
  private diffViewer: DiffViewer;

  constructor(diffViewer: DiffViewer) {
    this.diffViewer = diffViewer;
  }

  provideTextDocumentContent(uri: vscode.Uri): string {
    return this.diffViewer.getContent(uri) ?? '';
  }
}
