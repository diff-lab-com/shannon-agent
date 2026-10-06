/**
 * P0-3 — attachment refusal feedback.
 *
 * The backend never silently drops an attachment anymore: `send_message`
 * returns every refused path as `{ path, reason }` (partial success — the
 * message still goes through when at least one attachment is readable and
 * in scope), and `check_attachment_paths` classifies chips at attach time.
 * This module centralizes the reason → i18n mapping so the send-time toast
 * and the composer's chip tooltips can never disagree.
 */

import { toast } from 'sonner'
import { messageFor } from '@/i18n'
import type { RejectedAttachment, RejectedAttachmentReason } from '@/types'

/** i18n key of the human-readable reason clause, per backend reason tag. */
export const REJECT_REASON_MESSAGE_KEYS: Record<RejectedAttachmentReason, string> = {
  out_of_working_dir: 'chat.attach.reason.outOfWorkingDir',
  unresolvable: 'chat.attach.reason.unresolvable',
  too_large: 'chat.attach.reason.tooLarge',
  no_working_dir: 'chat.attach.reason.noWorkingDir',
  // R2-P1-2 — image formats the multimodal whitelist never sends
  // (svg/bmp/…), flagged by the preflight and refused on send.
  unsupported_type: 'chat.attach.reason.unsupportedType',
}

/** i18n message for one rejection reason (falls back to the raw tag). */
export function rejectionReasonMessage(reason: RejectedAttachmentReason): string {
  return messageFor(REJECT_REASON_MESSAGE_KEYS[reason] ?? reason)
}

/** Last path segment across POSIX and Windows separators. */
export function attachmentBasename(path: string): string {
  return path.split(/[/\\]/).pop() || path
}

/**
 * Toast one "«file» was not sent: «reason»" line per refused attachment.
 * Called after a successful send — refusals ride along with the response
 * and must never block or roll back the message itself.
 */
export function reportRejectedAttachments(rejected?: RejectedAttachment[]): void {
  if (!rejected || rejected.length === 0) return
  for (const r of rejected) {
    toast.warning(
      messageFor('chat.attach.rejected.toast', {
        name: attachmentBasename(r.path),
        reason: rejectionReasonMessage(r.reason),
      }),
    )
  }
}
