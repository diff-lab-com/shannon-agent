# Notifications P2/P3 — Next-Sprint Plan

Follow-up to `settings-redesign-pm-plan.md` §2 ("Events × Channels" matrix).
Phase 1 (N1 merge outbound + N5 disable dead Email) shipped via
`s2/notifications-p1`. This doc scopes **Phase 2** and **Phase 3** for the next
sprint. These are *plans*, not commitments — validate Phase 2 before starting
Phase 3 (scope-discipline per the PM plan §3).

> **Status (2026-06-28):** **Phase 2 shipped.**
> - §D (N4 master + DND) → PR #78 (`s2/notifications-p2-dnd`).
> - §A (N3 event toggles) → PR #79 (`s2/notifications-p2-events`, stacks on #78).
> - The Phase 2 "backend dependency" engine-tagging concern is **resolved**: the
>   engine `Notification` already carries `level` (`NotificationLevel`), and the
>   desktop only fires two event types itself via `fire_query_notification`
>   (`query_complete` / `query_error`). Gating keys off `level` — **no engine
>   change needed** (was task T6).
> - **Phase 2 design decisions of record:** DND uses **drop-during-window**
>   (queue+replay is deferred to P3); timezone is **system-local** via
>   `chrono::Local` (IANA selection is P3).
> - **Phase 3 not started** — see the §B/§C scope note below: much of
>   "multi-destination" already exists, so P3 needs re-scoping before build.

## Phase 2 — event-type control + DND (N3 + N4)

Goal: let the user say *"notify me about **these events**"* and mute them on a
schedule.

### §A — Notification events (rows of the matrix) — fixes N3
- Surface a checklist of **event types**: task completed, permission/request
  needs review, error/failure, agent message, routine triggered. Each toggleable.
- Persist per-event enable flags. The events themselves already exist as engine
  emit points (`NotificationKind` in `commands_notifications.rs` /
  `fire_query_notification`); the gap is there is no per-type on/off today — the
  webhook fires on whatever the backend decides.

### §D — Global controls — fixes N4
- **Master enable** toggle for all notifications.
- **Quiet hours / DND**: a time window (with timezone) during which desktop
  notifications are suppressed (queue or drop — decide during design; recommend
  *queue + replay on window end* so nothing is lost).

### Backend dependency (flag for `shannon-code`)
- Per-event routing (§A) likely needs engine support: the desktop shell only
  fans out what the engine emits. Either (a) the engine tags each notification
  with `NotificationKind` and the desktop filters, or (b) the desktop subscribes
  per-kind. Confirm the engine contract before UI work — this is the
  cross-cutting risk called out in PM plan §3.
- DND/quiet-hours is desktop-local (no engine change) — safe to build in
  parallel.

### Phase 2 scope estimate
- UI: new `EventsSection` (checklist) + DND controls in `NotificationsSettings`;
  build toward the matrix shell (rows ready, single "Desktop" column for now).
- i18n: new `settings.notifications.events.*` + `settings.notifications.dnd.*`
  keys (en + zh parity).
- Tests: event-toggle persistence; DND window evaluation.

## Phase 3 — multi-destination + delivery feedback (N2 + N7)

Goal: fan out to many destinations and confirm delivery.

> **Scope note (2026-06-28):** much of §B already exists. The desktop already
> supports multiple outbound destinations — webhook (Feishu / DingTalk / WeChat
> / Slack / custom URL) **and** direct Slack + Telegram bots — via
> `getOutboundConfig` / `saveOutboundConfig` / `sendOutboundTest`, plus the
> inbound wizard. So P3 is **not** "build destinations from scratch"; the real
> gaps are (1) **per-destination** test-send (today `sendOutboundTest` tests
> *all* configured providers at once) and (2) replacing the status **poll** with
> push/event-driven refresh (the `CONFIG_UPDATED` pattern already used
> elsewhere). Re-validate scope before building — do not duplicate the existing
> outbound config into a parallel roster.

### §B — Multi-destination — fixes N2
- Replace the single-webhook config with a **list of destinations** (Slack,
  Telegram, webhook groups), each with a connection dot. Mirrors the Models P2
  roster pattern (`ProvidersFile` / provider rows) — reuse that UX primitive.

### §C — Test-send + push-based status — fixes N7
- Per-destination **Test/Send-sample** button.
- Replace the 30s poll with push-based status (event-driven refresh, like the
  providers `CONFIG_UPDATED` pattern) + surface delivery-failure feedback.

### Backend dependency
- Multi-destination persistence is desktop-local (mirror `providers.json` → a
  `destinations.json`). Test-send reuses existing outbound test infra.
- Push-based status may need an engine event for delivery results — confirm.

### Phase 3 scope estimate
- UI: `DestinationsSection` (roster + modal, reuse Models P2 components) +
  test-send buttons + status rewrite.
- Tests: destination CRUD; test-send mock; status event handling.

## Sequencing recommendation

1. **Confirm the engine event-tagging contract** (does `NotificationKind`
   reach the desktop per-emit?) — unblocks Phase 2 §A. Do this first.
2. **Phase 2** DND (desktop-local, no engine dep) can ship immediately.
3. **Phase 2** event checklist once the contract is confirmed.
4. **Phase 3** after Phase 2 validates the matrix UX.

## Open questions for sprint planning
- ~~DND: queue-and-replay vs. drop-during-window?~~ **Resolved (P2):**
  drop-during-window. Queue+replay deferred to P3.
- ~~Event-type set: exactly which kinds (align with `NotificationKind` enum)?~~
  **Resolved (P2):** the desktop fires two event types today (`query_complete`,
  `query_error`); toggles are `on_completed` / `on_failed`, keyed off
  `NotificationLevel` (Error vs everything else). Permission / agent / routine
  toggles land when those sources are wired.
- Does the engine emit delivery results for Phase 3 push-based status, or is
  that net-new? **Still open** — confirm before P3 push-status work.
