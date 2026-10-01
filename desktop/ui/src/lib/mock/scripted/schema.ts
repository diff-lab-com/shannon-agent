// ChatScript schema — typed shape + ajv validation for the scripted mock
// backend (R1 chat-testing infra, plan §2.1).
//
// A script is authored as YAML in `desktop/ui/e2e/scripts/*.yaml`, loaded to
// plain JSON by the Playwright helper, and validated HERE with ajv before the
// player arms. Event names are the known EVENT_NAMES chat subset; payloads
// mirror the Rust wire shape (snake_case for `query:*`/`permission-request`,
// camelCase for `budget:*` — same split as the real backend).
//
// NOTE for the Playwright loader: this module must stay import-safe outside
// Vite (no `@/` alias, no `import.meta.env`) — e2e/helpers imports it
// directly by relative path.

import Ajv from 'ajv'

/** Known event names a script step may emit (chat-scoped EVENT_NAMES). */
export const SCRIPT_EVENT_NAMES = [
  'query:text',
  'query:thinking',
  'query:tool-start',
  'query:tool-progress',
  'query:tool-result',
  'query:notice',
  'query:usage',
  'query:completed',
  'query:failed',
  'query:cancelled',
  'permission-request',
  'budget:warning',
  'budget:exceeded',
] as const

export type ScriptEventName = (typeof SCRIPT_EVENT_NAMES)[number]

/** Events that settle a turn (player stops the turn after emitting them). */
export const TERMINAL_EVENTS: readonly string[] = [
  'query:completed',
  'query:failed',
  'query:cancelled',
]

/** A chunked streaming step: one emit per chunk, `chunkDelayMs` apart. */
export interface ScriptStep {
  event?: ScriptEventName
  /** Per-chunk content — each chunk is emitted as one `{ content }` event. */
  chunks?: string[]
  /** Gap between chunks in ms (default 30; scaled by `control.speed`). */
  chunkDelayMs?: number
  /** Payload merged (shallow) over the auto-generated one. */
  payload?: Record<string, unknown>
  /** Explicit UI stop point — the player pauses until `control.resume()`. */
  waitFor?: 'ui'
  /**
   * Known-bug anchor (R2 chat-testing plan §A). A step carrying this marker
   * is SKIPPED by the player (no event emitted) and annotated via
   * console.info — it pins where the journey is shaped by a tracked bug
   * (e.g. A-3 retry drops attachments, A-19 cancel discards partial text).
   * The YAML comment at the step documents the flip condition; when the fix
   * lands (R4), remove the marker — the step resumes executing and the
   * spec/L1 assertions next to it flip to the fixed behavior.
   */
  knownIssue?: string
}

export interface ScriptSeedMessage {
  role: 'user' | 'assistant'
  content: string
  attachments?: string[]
}

export interface ScriptSeedSession {
  id: string
  title: string
  messages: ScriptSeedMessage[]
}

export interface ScriptSeed {
  config?: {
    provider?: string
    hasKey?: boolean
    budgetUsd?: number | null
  }
  sessions?: ScriptSeedSession[]
}

/** What `cancel_query` does mid-turn. Default: emit `query:cancelled` now. */
export interface ScriptOnCancel {
  emit: ScriptStep[]
}

export interface ChatScript {
  name: string
  description?: string
  seed?: ScriptSeed
  turns: Array<{
    user: string
    attachments?: string[]
    script: ScriptStep[]
  }>
  onCancel?: ScriptOnCancel
}

/** ajv schema mirroring the types above (JSON-schema draft-07). */
export const chatScriptSchema = {
  type: 'object',
  required: ['name', 'turns'],
  additionalProperties: false,
  properties: {
    name: { type: 'string', minLength: 1 },
    description: { type: 'string' },
    seed: {
      type: 'object',
      additionalProperties: false,
      properties: {
        config: {
          type: 'object',
          additionalProperties: false,
          properties: {
            provider: { type: 'string' },
            hasKey: { type: 'boolean' },
            budgetUsd: { type: ['number', 'null'] },
          },
        },
        sessions: {
          type: 'array',
          items: {
            type: 'object',
            required: ['id', 'title', 'messages'],
            additionalProperties: false,
            properties: {
              id: { type: 'string', minLength: 1 },
              title: { type: 'string' },
              messages: {
                type: 'array',
                items: {
                  type: 'object',
                  required: ['role', 'content'],
                  additionalProperties: false,
                  properties: {
                    role: { enum: ['user', 'assistant'] },
                    content: { type: 'string' },
                    attachments: { type: 'array', items: { type: 'string' } },
                  },
                },
              },
            },
          },
        },
      },
    },
    turns: {
      type: 'array',
      minItems: 1,
      items: {
        type: 'object',
        required: ['user', 'script'],
        additionalProperties: false,
        properties: {
          user: { type: 'string' },
          attachments: { type: 'array', items: { type: 'string' } },
          script: {
            type: 'array',
            items: {
              type: 'object',
              additionalProperties: false,
              properties: {
                event: { enum: [...SCRIPT_EVENT_NAMES] },
                chunks: { type: 'array', items: { type: 'string' }, minItems: 1 },
                chunkDelayMs: { type: 'number', minimum: 0 },
                payload: { type: 'object' },
                waitFor: { enum: ['ui'] },
                knownIssue: { type: 'string', minLength: 1 },
              },
              anyOf: [
                { required: ['event'] },
                { required: ['waitFor'] },
              ],
            },
          },
        },
      },
    },
    onCancel: {
      type: 'object',
      required: ['emit'],
      additionalProperties: false,
      properties: {
        emit: {
          type: 'array',
          items: {
            type: 'object',
            additionalProperties: false,
            properties: {
              event: { enum: [...SCRIPT_EVENT_NAMES] },
              chunks: { type: 'array', items: { type: 'string' }, minItems: 1 },
              chunkDelayMs: { type: 'number', minimum: 0 },
              payload: { type: 'object' },
              knownIssue: { type: 'string', minLength: 1 },
            },
          },
        },
      },
    },
  },
} as const

export interface ValidationResult {
  ok: boolean
  errors: string[]
}

let ajvSingleton: Ajv | null = null

/** Validate an unknown JSON value as a ChatScript. Never throws. */
export function validateScript(json: unknown): ValidationResult {
  try {
    ajvSingleton = ajvSingleton ?? new Ajv({ allErrors: true })
    const validate = ajvSingleton.compile(chatScriptSchema)
    const ok = validate(json) as boolean
    if (ok) return { ok: true, errors: [] }
    const errors = (validate.errors ?? []).map(e => `${e.instancePath || '/'} ${e.message ?? 'is invalid'}`)
    return { ok: false, errors }
  } catch (e) {
    return { ok: false, errors: [String(e)] }
  }
}
