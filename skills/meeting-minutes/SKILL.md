---
name: Meeting Minutes
description: Turn a meeting transcript (.srt/.vtt/.txt) into a structured minutes document — Summary, Decisions, Action Items with owners, Open Questions — delivered as Markdown on disk. No audio processing; the transcript must already exist as text. 会议转写纪要整理（仅文本转写，不处理录音）。
when_to_use: Use when the user provides a meeting transcript (.srt, .vtt, or .txt) and asks for meeting notes, minutes, or a recap.
argument-hint: "[path-to-transcript-or-meeting-topic]"
allowed-tools:
  - Bash
  - Read
  - Write
user-invocable: true
---

# Meeting Minutes

Turn a text transcript into a real minutes document on disk — a Markdown
file the user can read, edit, and share. **This skill never processes audio
or video**: if the user drops a `.mp3`/`.wav`/`.mp4` recording or asks you to
"listen to" a call, stop and tell them plainly that transcription is out of
scope — they need to supply a transcript (`.srt`, `.vtt`, `.txt`, or pasted
text) from their recorder, conferencing tool, or a speech-to-text service.

The core path uses **only the Read and Write tools** — zero external
dependencies, so it works on any host.

## Step 1: Collect inputs

Ask the user (skip any question they already answered; if they said "just do
it", pick sensible defaults and state them):

1. **Transcript source** — a file path, or text pasted into the chat. Read
   the file with the Read tool before anything else; if it is clearly not a
   transcript (empty, binary, unrelated content), say so and stop.
2. **Format check** — `.srt`/`.vtt` cues are flattened into speaker-tagged
   lines first (timestamps dropped, text kept); `.txt` is used as-is. Long
   transcripts are read in chunks; state how many chunks you processed.
3. **Attendees and meeting name**, if the transcript does not make them
   obvious. Never invent attendee names — derive owners only from what the
   transcript says ("Alice will send the spec").
4. **Language** of the minutes — match the transcript's language by default.
5. **Output filename** — default
   `output/minutes-<YYYYMMDD>-<short-slug>.md`.

If the user passed `${0}`, treat it as the transcript path or meeting topic.

## Step 2: Extract the four sections

Produce exactly these headings, in this order:

1. **Summary** — 3-6 sentences: what the meeting was about and where it
   landed. Facts only; mark inferences as inferences.
2. **Decisions** — one bullet per decision, each traceable to a specific
   exchange. If the transcript shows debate without a resolution, that is an
   Open Question, not a Decision.
3. **Action Items** — a table: | Action | Owner | Due |. Include an owner
   and a due date only when the transcript states (or makes unambiguous)
   them — otherwise write `unassigned` / `TBD`. Never guess a person or a
   date. No transcript evidence means the item does not belong here.
4. **Open Questions** — unresolved points, each with who raised it when
   attributable.

Discard filler, small talk, and repeated statements. If the transcript is
too noisy to support a section, keep the heading and state honestly that
the transcript does not support it.

## Step 3: Suggest follow-ups (only when asked)

If the user asks to "track" or "follow up on" the action items, suggest
creating a scheduled reminder with the **Cron** tool (`cron_create` — e.g. a
weekly review that re-reads the minutes file and reports open items). Create
the cron task only on explicit confirmation, and repeat its schedule back to
the user.

## Step 4: Write the output

All artifacts go to the **`output/` directory at the project root** (create
it if missing). Default name:
`output/minutes-<YYYYMMDD>-<short-slug>.md`. Write the Markdown with the
Write tool. Never write outside the project root unless the user gives an
absolute path.

## Step 5: Verify the artifact

Re-read the written file and check: the four headings are present and in
order, every Action Item has an owner cell and a due cell (even if the value
is `unassigned`/`TBD`), and no action item, decision, or question lacks a
basis in the transcript. If the user gave a path, confirm the file exists on
disk before claiming success — never report a document you did not verify.

## Step 6: Report results honestly

Tell the user: the output path, which transcript you used (and its length),
how many summary/decision/action/question entries each section carries, and
anything you could not attribute (owners, dates) rather than papering over
it.

## Failure and retry rules

1. If the transcript is missing, unreadable, or empty, stop and say so — do
   not fabricate minutes.
2. If the file is an audio/video format, explain that this skill needs a
   text transcript and name the formats it accepts.
3. If attribution is ambiguous ("I'll send it" — who is "I"?), mark the
   owner `unassigned` and list the ambiguity under Open Questions.
4. Retry at most **2** times. After the second failure, stop and report
   what failed and what you tried.
5. If Step 5 fails, do not ship the file: fix or delete the broken artifact
   and say so. Never report success unless the verification passed.

## Not supported in v1

Honest scope — this skill restructures text the user already has. It does
**not** support: audio/video transcription, speaker diarization, real-time
meeting bots, translation, or docx/pdf export (route those to the
docx-report skill or a conversion tool if the user needs a binary format).
Say so up front instead of accepting a recording and failing later.
