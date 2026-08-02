/**
 * P2-5a spike — mock Tauri events.
 *
 * Three canned messages that exercise the three assistant-ui message shape
 * branches the real Shannon Tauri stream will eventually emit:
 *
 *   1. A user text message (basic round-trip)
 *   2. An assistant text message (basic assistant response)
 *   3. An assistant message with a tool-call part (Shannon's bash / read /
 *      glob family — tools surface as `tool-call` parts, not as separate
 *      messages).
 *
 * Used only by `ChatV2Spike.tsx` — production `Chat.tsx` is unchanged.
 *
 * Important: the messages use `metadata.custom: {}` (required by `BaseThreadMessage`
 * in @assistant-ui/core@0.3.2). Without it `noUnusedLocals` would fail during
 * TypeScript's strict metadata-property enforcement at the role-discriminated
 * union level.
 */
import type { ThreadMessage } from '@assistant-ui/react';

export const MOCK_MESSAGES: readonly ThreadMessage[] = [
  {
    id: 'mock-1',
    role: 'user',
    content: [{ type: 'text', text: '(spike) hello from a mock user' }],
    attachments: [],
    createdAt: new Date('2026-08-01T00:00:00Z'),
    metadata: { custom: {} },
  },
  {
    id: 'mock-2',
    role: 'assistant',
    status: { type: 'complete', reason: 'stop' },
    content: [{ type: 'text', text: '(spike) hello back from a mock assistant' }],
    createdAt: new Date('2026-08-01T00:00:01Z'),
    metadata: {
      unstable_state: null,
      unstable_annotations: [],
      unstable_data: [],
      steps: [],
      custom: {},
    },
  },
  {
    id: 'mock-3',
    role: 'assistant',
    status: { type: 'complete', reason: 'stop' },
    content: [
      {
        type: 'tool-call',
        toolCallId: 'tool-1',
        toolName: 'bash',
        args: { command: 'ls -la' },
        argsText: '{"command":"ls -la"}',
      },
    ],
    createdAt: new Date('2026-08-01T00:00:02Z'),
    metadata: {
      unstable_state: null,
      unstable_annotations: [],
      unstable_data: [],
      steps: [],
      custom: {},
    },
  },
];
