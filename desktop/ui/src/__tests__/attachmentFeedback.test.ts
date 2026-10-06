// G3 P0-3 — refused attachments must be VISIBLE.
//
// The backend's `send_message` now returns `rejected_attachments` alongside
// a successful send (partial success). `reportRejectedAttachments` toasts
// one "«file» was not sent: «reason»" line per refusal — this is the fix
// for the old silently-dropped chip.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { toast } from 'sonner'
import {
  reportRejectedAttachments,
  rejectionReasonMessage,
  attachmentBasename,
} from '@/lib/attachmentFeedback'
import type { RejectedAttachment } from '@/types'

vi.mock('sonner', () => ({
  toast: {
    warning: vi.fn(),
    error: vi.fn(),
    success: vi.fn(),
  },
}))

describe('reportRejectedAttachments (G3 P0-3)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('toasts one "<name> was not sent: <reason>" line per refused file', () => {
    const rejected: RejectedAttachment[] = [
      { path: '/home/u/Downloads/report.pdf', reason: 'out_of_working_dir' },
      { path: 'C:\\Users\\u\\notes.tmp', reason: 'unresolvable' },
    ]
    reportRejectedAttachments(rejected)

    expect(toast.warning).toHaveBeenCalledTimes(2)
    const first = vi.mocked(toast.warning).mock.calls[0][0] as string
    const second = vi.mocked(toast.warning).mock.calls[1][0] as string
    expect(first).toBe('report.pdf was not sent: it is outside the working directory Shannon may read')
    // Windows paths split like POSIX ones for the display name.
    expect(second).toBe('notes.tmp was not sent: the file could not be read (missing or unreadable)')
  })

  it('covers every reason tag the backend can send', () => {
    for (const [reason, key] of Object.entries({
      out_of_working_dir: 'outside the working directory',
      unresolvable: 'could not be read',
      too_large: 'size limit',
      no_working_dir: 'working directory is set',
      unsupported_type: 'image format',
    })) {
      const msg = rejectionReasonMessage(reason as RejectedAttachment['reason'])
      expect(msg, reason).toContain(key)
    }
    reportRejectedAttachments([
      { path: '/tmp/big.png', reason: 'too_large' },
      { path: '/tmp/x', reason: 'no_working_dir' },
    ])
    expect(toast.warning).toHaveBeenCalledTimes(2)
  })

  it('is a no-op when nothing was refused (and when the field is absent)', () => {
    reportRejectedAttachments(undefined)
    reportRejectedAttachments([])
    expect(toast.warning).not.toHaveBeenCalled()
  })

  it('formats the display name from the last path segment', () => {
    expect(attachmentBasename('/a/b/c.txt')).toBe('c.txt')
    expect(attachmentBasename('C:\\a\\b\\c.txt')).toBe('c.txt')
    expect(attachmentBasename('')).toBe('')
  })
})
