// ScriptedBackend player — replays a ChatScript's event turns through the
// mock event bridge (R1 chat-testing infra, plan §2.1/§2.2).
//
// State machine (per script):
//   idle ──load──▶ armed ──send_message──▶ playing ⇄ waitingUi/waitingPermission
//                  ▲ ▲                        │
//                  │ └────── next turn ◀──────┘ (terminal event / steps end)
//                  └── done (turns exhausted → send_message falls back to the
//                          default mock handler, i.e. pre-script behavior)
//
// Contract notes (brief §B):
//  • send_message with the player armed consumes the NEXT turn in order —
//    optimistic UI semantics belong to the real UI, the player only emits.
//  • `query:text` steps replay per-chunk with `chunkDelayMs` gaps (default
//    30ms) scaled by `control.speed`.
//  • `waitFor: 'ui'` and `permission-request` both PARK the player;
//    permission-request additionally pauses automatically after emitting —
//    real-backend semantics (the engine waits for `respond_permission`).
//  • Terminal events (query:completed|failed|cancelled) settle the turn; a
//    turn whose steps end without one auto-emits `query:completed` so a
//    under-specified script can never leave the composer spinning forever.
//  • Every auto-emitted payload gets `{ query_id: 'q-<turn>', session_id }`
//    (session_id = the send_message target; null keeps AppContext's
//    window-local routing). `budget:*` is the exception: the frozen backend
//    shape is camelCase `{ sessionId, spentUsd, budgetUsd }`.
//  • The step's `payload` shallow-merges OVER the auto payload.
//
// The player is DOM-free: emissions and seeding go through the injected
// PlayerRuntime, so unit tests capture events without a browser.

import type { ChatScript, ScriptSeed, ScriptStep } from './schema'
import { TERMINAL_EVENTS, validateScript } from './schema'
import { setScriptSeed } from './seed'

export type PlayerPhase =
  | 'idle'             // no script loaded
  | 'armed'            // script loaded, waiting for the next send_message
  | 'playing'          // emitting steps
  | 'waitingUi'        // parked at waitFor:'ui' or a pauseAt marker
  | 'waitingPermission' // parked after permission-request
  | 'done'             // all turns consumed

export interface PlayerRuntime {
  /** Deliver one event to the app (prod: eventBridge.dispatchEvent). */
  emit(event: string, payload: Record<string, unknown>): void
  /** Arm/clear the handlers seed override (prod: setScriptSeed). */
  onSeed(seed: ScriptSeed | null): void
  /** Time-delayed callback → cancel fn. Prod: setTimeout. */
  schedule(delayMs: number, fn: () => void): () => void
}

/** The `send_message` invoke args the app actually sends (tauri-api.ts). */
export interface SendArgs {
  message?: string
  filePaths?: string[] | null
  budgetBypass?: boolean | null
  sessionId?: string | null
}

/** One scripted send's observed args (snapshot `sends` log entry). */
export interface SendRecord {
  /** Which script turn consumed this send (-1 when the script is exhausted). */
  turnIndex: number
  message: string | null
  attachments: string[] | null
  budgetBypass: boolean
  sessionId: string | null
}

export const DEFAULT_CHUNK_DELAY_MS = 30

/** Neutral `budget:*` defaults — ≥80% warning AND ≥100% exceeded plausible. */
const DEFAULT_BUDGET_SPENT = 4.2
const DEFAULT_BUDGET_CAP = 5

interface ActiveTurn {
  turnIndex: number
  stepIndex: number
  queryId: string
  sessionId: string | null
  cancelTimer: (() => void) | null
}

function defaultSchedule(delayMs: number, fn: () => void): () => void {
  const t = setTimeout(fn, delayMs)
  return () => clearTimeout(t)
}

export class ScriptPlayer {
  private runtime: PlayerRuntime
  private script: ChatScript | null = null
  private phase: PlayerPhase = 'idle'
  private turn: ActiveTurn | null = null
  private turnCounter = 0
  /** pauseAt marker — parks (without running) when stepIndex reaches it. */
  private pauseAtStep: number | null = null
  /** Why the player last parked — decides what resume() does next. */
  private parkKind: 'marker' | 'waitFor' | 'permission' | null = null
  private pendingSends: Array<{ sessionId: string | null }> = []
  private speedValue = 1
  /** Every `respond_permission` observed while scripted (assertion log). */
  private permissionLog: Array<Record<string, unknown>> = []
  /** Every scripted send's observed args (R3 assertion log). */
  private sends: SendRecord[] = []

  constructor(runtime?: Partial<PlayerRuntime>) {
    this.runtime = {
      emit: runtime?.emit ?? (() => {}),
      onSeed: runtime?.onSeed ?? setScriptSeed,
      schedule: runtime?.schedule ?? defaultSchedule,
    }
  }

  // ── lifecycle ────────────────────────────────────────────────────────────

  /** Validate + arm a script. Re-load mid-turn cancels the current turn. */
  load(json: unknown): { ok: boolean; errors: string[] } {
    const result = validateScript(json)
    if (!result.ok) return result
    this.haltTurn()
    const script = json as ChatScript
    this.script = script
    this.turnCounter = 0
    this.turn = null
    this.pendingSends = []
    this.permissionLog = []
    this.sends = []
    this.pauseAtStep = null
    this.parkKind = null
    this.phase = 'armed'
    this.runtime.onSeed(script.seed ?? null)
    return { ok: true, errors: [] }
  }

  /** Back to the pre-script world: default seed data, no interception. */
  reset(): void {
    this.haltTurn()
    this.script = null
    this.turnCounter = 0
    this.turn = null
    this.pendingSends = []
    this.permissionLog = []
    this.sends = []
    this.pauseAtStep = null
    this.parkKind = null
    this.speedValue = 1
    this.phase = 'idle'
    this.runtime.onSeed(null)
  }

  private haltTurn(): void {
    if (this.turn?.cancelTimer) {
      this.turn.cancelTimer()
      this.turn.cancelTimer = null
    }
  }

  // ── command hooks (called from coreMock.invoke) ─────────────────────────

  /**
   * `send_message` interception. Returns the scripted response while turns
   * remain, or null to fall through to the default handler (pre-script
   * behavior — no events, immediate query id).
   *
   * R3: the turn's `rejectedAttachments` ride along on the response (the
   * backend's P0-3 partial-success shape) and every scripted send's args
   * land on the snapshot's `sends` log — the assertion surface the
   * budget-bypass / attachment-preservation anchors read.
   *
   * A-1 anchor (R4 group 2): a `sendRejects` turn makes the invoke REJECT —
   * the scripted counterpart of the real backend's pre-turn guards (budget
   * / concurrent-query / goal-owned). The throw happens BEFORE any turn
   * starts, so no events are emitted and no query id is allocated; the turn
   * still counts as consumed, so the next send plays the following one.
   */
  handleSendMessage(args: SendArgs | undefined): { query_id: string; rejected_attachments?: unknown[] } | null {
    if (!this.script || this.phase === 'idle' || this.phase === 'done') return null
    if (this.phase !== 'armed') {
      // A send while a turn is still open cannot be scripted (the real
      // backend rejects concurrent queries); fall through to the default.
      return null
    }
    const message = typeof args?.message === 'string' ? args.message : null
    const attachments = Array.isArray(args?.filePaths) ? (args!.filePaths as unknown[]).filter(f => typeof f === 'string') : null
    const turnIndex = this.turnCounter
    this.sends.push({
      turnIndex,
      message,
      attachments,
      budgetBypass: args?.budgetBypass === true,
      sessionId: (args?.sessionId ?? null) as string | null,
    })
    const sessionId = (args?.sessionId ?? null) as string | null
    if (this.script.turns[turnIndex]?.sendRejects) {
      this.turnCounter += 1
      if (this.turnCounter >= this.script.turns.length) this.phase = 'done'
      throw new Error(`send rejected by scripted backend guard (turn ${turnIndex})`)
    }
    // startTurn returns the id up front — a turn whose steps settle
    // synchronously (e.g. a lone query:completed) is already finished by the
    // time startTurn returns, so this.turn is no longer readable.
    const queryId = this.startTurn(sessionId)
    const rejected = this.script.turns[turnIndex]?.rejectedAttachments
    return rejected && rejected.length > 0
      ? {
          query_id: queryId,
          rejected_attachments: rejected.map(r => ({ path: r.path, reason: r.reason })),
        }
      : { query_id: queryId }
  }

  /**
   * `cancel_query` interception. Mid-turn: play the script's `onCancel`
   * steps (default: emit `query:cancelled` immediately) and settle. Returns
   * true when a turn was actually cancelled.
   */
  handleCancelQuery(): boolean {
    if (this.phase !== 'playing' && this.phase !== 'waitingUi' && this.phase !== 'waitingPermission') {
      return false
    }
    this.haltTurn()
    const steps = this.script?.onCancel?.emit
    if (steps && steps.length > 0) {
      this.playCancelSteps(steps)
    } else {
      this.emitEvent('query:cancelled')
      this.finishTurn()
    }
    return true
  }

  /**
   * `respond_permission` interception — always recorded (assertion log);
   * resumes a permission-parked turn. Returns true when it resumed one.
   */
  handleRespondPermission(args: Record<string, unknown>): boolean {
    this.permissionLog.push({ ...args, at: Date.now() })
    if (this.phase === 'waitingPermission') {
      this.resume()
      return true
    }
    return false
  }

  // ── test console (window.__shannonMock.control) ─────────────────────────

  /** Resume a parked turn (waitFor:'ui' / permission-request / pauseAt). */
  resume(): void {
    const turn = this.turn
    if (!turn || (this.phase !== 'waitingUi' && this.phase !== 'waitingPermission')) return
    const kind = this.parkKind
    this.parkKind = null
    this.phase = 'playing'
    if (kind !== 'marker') {
      // The parked step itself did its job (waitFor marker / permission
      // emitted) — move past it. A pauseAt marker parked BEFORE its step,
      // so that one still has to run.
      turn.stepIndex += 1
    }
    this.advance()
  }

  /** Arm a pause marker: park (without running) when stepIndex hits `i`. */
  pauseAt(i: number): void {
    this.pauseAtStep = i
  }

  get speed(): number {
    return this.speedValue
  }

  set speed(v: number) {
    if (typeof v === 'number' && Number.isFinite(v) && v > 0) this.speedValue = v
  }

  /** Emit an event NOW through the bridge, bypassing the player state. */
  emitNow(event: string, payload: Record<string, unknown>): void {
    this.runtime.emit(event, payload)
  }

  /** Test/console introspection. */
  snapshot(): {
    phase: PlayerPhase
    turnIndex: number | null
    stepIndex: number | null
    /** send_message count consumed by the script since load (R2). */
    sentTurns: number
    permissionLog: Array<Record<string, unknown>>
    /** Observed args of every scripted send since load (R3). */
    sends: SendRecord[]
    speed: number
  } {
    return {
      phase: this.phase,
      turnIndex: this.turn?.turnIndex ?? null,
      stepIndex: this.turn?.stepIndex ?? null,
      sentTurns: this.turnCounter,
      permissionLog: [...this.permissionLog],
      sends: this.sends.map(s => ({ ...s, attachments: s.attachments ? [...s.attachments] : null })),
      speed: this.speedValue,
    }
  }

  // ── engine ───────────────────────────────────────────────────────────────

  private startTurn(sessionId: string | null): string {
    const turnIndex = this.turnCounter
    this.turnCounter += 1
    const turn: ActiveTurn = {
      turnIndex,
      stepIndex: 0,
      queryId: `q-${turnIndex}`,
      sessionId,
      cancelTimer: null,
    }
    this.turn = turn
    this.phase = 'playing'
    this.parkKind = null
    this.advance()
    return turn.queryId
  }

  /** Auto payload for the turn in flight (budget:* events never use this). */
  private autoPayload(extra?: Record<string, unknown>): Record<string, unknown> {
    const base: Record<string, unknown> = {
      query_id: this.turn?.queryId ?? null,
      session_id: this.turn?.sessionId ?? null,
    }
    return extra ? { ...base, ...extra } : base
  }

  private budgetAutoPayload(extra?: Record<string, unknown>): Record<string, unknown> {
    const seed = this.script?.seed?.config
    const base: Record<string, unknown> = {
      sessionId: this.turn?.sessionId ?? null,
      // R3: an explicitly seeded spend wins (the over-budget journey's
      // events must agree with what get_session_usage reports); otherwise
      // the 84%-of-cap warning default, then the neutral pair.
      spentUsd: seed?.spentUsd
        ?? (seed?.budgetUsd != null ? seed.budgetUsd * 0.84 : DEFAULT_BUDGET_SPENT),
      budgetUsd: seed?.budgetUsd ?? DEFAULT_BUDGET_CAP,
    }
    return extra ? { ...base, ...extra } : base
  }

  private emitEvent(event: string, extra?: Record<string, unknown>): void {
    const payload = event === 'budget:warning' || event === 'budget:exceeded'
      ? this.budgetAutoPayload(extra)
      : this.autoPayload(extra)
    this.runtime.emit(event, payload)
  }

  /**
   * Run steps synchronously until the turn parks, ends, or hits a chunked
   * step (whose remaining chunks ride the scheduler).
   */
  private advance(): void {
    const turn = this.turn
    const script = this.script
    if (!turn || !script) return
    for (;;) {
      if (this.pauseAtStep === turn.stepIndex) {
        this.pauseAtStep = null
        this.parkKind = 'marker'
        this.phase = 'waitingUi'
        return
      }
      const steps = script.turns[turn.turnIndex]?.script ?? []
      if (turn.stepIndex >= steps.length) {
        // No terminal event scripted — settle like the real backend would.
        this.finishTurn('query:completed')
        return
      }
      const step = steps[turn.stepIndex]

      if (step.knownIssue != null) {
        // Known-bug anchor (schema §A): skip execution, leave a console
        // trace so a live demo session shows why the step didn't run.
        console.info(
          `[shannon-mock] knownIssue ${step.knownIssue}: turn ${turn.turnIndex} step ${turn.stepIndex}`
            + ` (${step.event ?? 'waitFor'}${step.chunks ? ` ×${step.chunks.length}` : ''}) skipped`
            + ' — anchored known bug; remove the marker to flip the journey',
        )
        turn.stepIndex += 1
        continue
      }
      if (step.waitFor === 'ui') {
        this.parkKind = 'waitFor'
        this.phase = 'waitingUi'
        return
      }
      if (step.event === 'permission-request') {
        this.phase = 'playing'
        this.emitEvent(step.event, step.payload)
        // Real-backend semantics: the engine pauses until respond_permission.
        this.parkKind = 'permission'
        this.phase = 'waitingPermission'
        return
      }
      if (step.chunks && step.chunks.length > 0) {
        this.phase = 'playing'
        const gapMs = (step.chunkDelayMs ?? DEFAULT_CHUNK_DELAY_MS) / this.speedValue
        const chunks = step.chunks
        let i = 0
        const emitChunk = () => {
          if (!this.turn || this.turn !== turn) return // reset() mid-chunks
          // step.payload shallow-merges OVER the auto payload (brief §B),
          // including `content` — author intent wins over the chunk text.
          this.emitEvent(step.event!, { content: chunks[i], ...(step.payload ?? {}) })
          i += 1
          if (i < chunks.length) {
            turn.cancelTimer = this.runtime.schedule(gapMs, emitChunk)
          } else {
            turn.cancelTimer = null
            turn.stepIndex += 1
            this.advance()
          }
        }
        emitChunk()
        return
      }
      // Plain payload-only step.
      const event = step.event!
      this.phase = 'playing'
      this.emitEvent(event, step.payload)
      if (TERMINAL_EVENTS.includes(event)) {
        this.finishTurn()
        return
      }
      turn.stepIndex += 1
    }
  }

  private finishTurn(autoEvent?: string): void {
    this.haltTurn()
    if (autoEvent) this.emitEvent(autoEvent)
    this.turn = null
    this.parkKind = null
    if (this.turnCounter >= (this.script?.turns.length ?? 0)) {
      this.phase = 'done'
      return
    }
    this.phase = 'armed'
    const next = this.pendingSends.shift()
    if (next) this.startTurn(next.sessionId)
  }

  /** onCancel steps: sequential, chunks/delays honored, pauses forbidden. */
  private playCancelSteps(steps: ScriptStep[]): void {
    const turn = this.turn!
    this.phase = 'playing'
    // Flatten to emissions first so the delay of each item is known up front.
    const flat = steps.flatMap(step => {
      if (step.knownIssue != null) {
        // Known-bug anchor: skipped like turn steps; if that drops the only
        // terminal event, the fallback below still settles the turn.
        console.info(
          `[shannon-mock] knownIssue ${step.knownIssue}: onCancel step (${step.event ?? 'waitFor'}) skipped`
            + ' — anchored known bug; remove the marker to flip the journey',
        )
        return []
      }
      if (step.chunks && step.chunks.length > 0) {
        const gap = step.chunkDelayMs ?? DEFAULT_CHUNK_DELAY_MS
        return step.chunks.map((content, i) => ({
          event: step.event!,
          payload: { ...(step.payload ?? {}), content },
          delayMs: i === 0 ? 0 : gap,
        }))
      }
      return step.event ? [{ event: step.event, payload: { ...(step.payload ?? {}) }, delayMs: 0 }] : []
    })
    let idx = 0
    let sawTerminal = false
    const next = () => {
      if (this.turn !== turn) return // reset() mid-cancel
      if (idx >= flat.length) {
        // A cancel script that never settles would strand the composer in
        // isQuerying — guarantee a terminal event like the default branch.
        if (!sawTerminal) this.emitEvent('query:cancelled')
        this.finishTurn()
        return
      }
      const item = flat[idx]
      idx += 1
      if (TERMINAL_EVENTS.includes(item.event)) sawTerminal = true
      this.emitEvent(item.event, item.payload)
      // Zero gaps chain synchronously — a cancel playback with no explicit
      // delays settles within one tick, like the default branch.
      if (item.delayMs > 0) {
        turn.cancelTimer = this.runtime.schedule(item.delayMs / this.speedValue, next)
      } else {
        next()
      }
    }
    next()
  }
}
