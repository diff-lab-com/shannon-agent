/**
 * FileApply — Applies accepted file changes to the workspace.
 *
 * Takes PendingChange objects that were accepted through the diff viewer
 * and writes them to disk using VS Code's workspace API.
 */

import * as vscode from 'vscode';
import * as fs from 'fs';
import * as path from 'path';
import { PendingChange } from './diffViewer';

/** Result of applying a single file change. */
export interface ApplyResult {
  filePath: string;
  success: boolean;
  error?: string;
}

/**
 * Applies accepted file changes to workspace files.
 *
 * For Edit tool changes, performs string replacement on the existing file.
 * For Write tool changes, writes the full proposed content.
 */
export async function applyChange(change: PendingChange): Promise<ApplyResult> {
  try {
    const dir = path.dirname(change.filePath);

    // Ensure parent directories exist
    if (!fs.existsSync(dir)) {
      fs.mkdirSync(dir, { recursive: true });
    }

    if (change.toolName === 'Write') {
      // Write tool — write full content
      fs.writeFileSync(change.filePath, change.proposedContent, 'utf-8');
    } else if (change.toolName === 'Edit') {
      // Edit tool — replace old_string with new_string
      const editMeta = (change as PendingChange & {
        editMeta?: { oldString: string; newString: string };
      }).editMeta;

      if (editMeta) {
        let currentContent = '';
        try {
          currentContent = fs.readFileSync(change.filePath, 'utf-8');
        } catch {
          // File may not exist; treat as empty
        }

        const updatedContent = currentContent.replace(
          editMeta.oldString,
          editMeta.newString
        );

        if (updatedContent === currentContent && editMeta.oldString !== '') {
          // The replacement didn't change anything — old_string may not match
          return {
            filePath: change.filePath,
            success: false,
            error: `old_string not found in ${change.filePath}`,
          };
        }

        fs.writeFileSync(change.filePath, updatedContent, 'utf-8');
      } else {
        // Fallback: no edit metadata, just write proposed content
        fs.writeFileSync(change.filePath, change.proposedContent, 'utf-8');
      }
    } else {
      return {
        filePath: change.filePath,
        success: false,
        error: `Unknown tool: ${change.toolName}`,
      };
    }

    // Open the file in the editor so the user can see the result
    const doc = await vscode.workspace.openTextDocument(change.filePath);
    await vscode.window.showTextDocument(doc, { preserveFocus: true });

    return { filePath: change.filePath, success: true };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return { filePath: change.filePath, success: false, error: message };
  }
}

/**
 * Apply multiple changes in sequence. Shows a summary notification at the end.
 */
export async function applyChanges(
  changes: PendingChange[]
): Promise<ApplyResult[]> {
  const results: ApplyResult[] = [];

  for (const change of changes) {
    const result = await applyChange(change);
    results.push(result);
  }

  const succeeded = results.filter((r) => r.success).length;
  const failed = results.filter((r) => !r.success).length;

  if (failed === 0) {
    vscode.window.showInformationMessage(
      `Shannon: Applied ${succeeded} file change(s) successfully.`
    );
  } else {
    vscode.window.showWarningMessage(
      `Shannon: Applied ${succeeded} change(s), ${failed} failed.`
    );
  }

  return results;
}

/**
 * Show a confirmation dialog before applying changes.
 * Returns true if the user confirms.
 */
export async function confirmApply(
  changes: readonly PendingChange[]
): Promise<boolean> {
  if (changes.length === 0) {
    return false;
  }

  const fileNames = changes
    .map((c) => path.basename(c.filePath))
    .join(', ');

  const action = await vscode.window.showWarningMessage(
    `Apply ${changes.length} file change(s): ${fileNames}?`,
    { modal: true },
    'Apply'
  );

  return action === 'Apply';
}
