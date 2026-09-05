import { type ChannelAdapter, type Logger, type ReplyTarget } from "../adapters/types.js";
import { type TurnContext, type TurnHandler } from "./types.js";

/**
 * Task lifecycle push (P1-4): report 任务开始 / 任务完成 / 任务失败 back to the
 * IM channel, in the outbound-notifier template spirit (short emoji-tagged
 * status lines) already used by the adapters' approval prompts.
 *
 * The reporter is fire-and-forget: a failed push is logged and never fails the
 * turn itself — the engine answer matters more than the status stamps.
 */

export interface TurnReporter {
  /** Fired when the turn begins (before the engine query runs). */
  started(): void;
  /** Fired after the final reply was delivered for a completed turn. */
  completed(): void;
  /** Fired when the engine reported a failure for this turn. */
  failed(error: string): void;
}

/** Derive a short task title from the message text (first N chars, collapsed). */
export function titleFromText(text: string, max = 30): string {
  const t = text.replace(/\s+/g, " ").trim();
  return t.length > max ? `${t.slice(0, max)}…` : t;
}

export function formatTaskStarted(title: string): string {
  return `🚀 已开始任务：${title}`;
}

export function formatTaskCompleted(title: string): string {
  return `✅ 任务完成：${title}`;
}

export function formatTaskFailed(title: string, error: string): string {
  return `❌ 任务失败：${title}\n${error}`;
}

/** Build a reporter that pushes lifecycle stamps to one reply target. */
export function createTurnReporter(opts: {
  adapter: ChannelAdapter;
  replyTarget: ReplyTarget;
  logger: Logger;
  title: string;
}): TurnReporter {
  const push = (text: string): void => {
    opts.adapter
      .send(opts.replyTarget, text)
      .catch((err: unknown) =>
        opts.logger.warn(`task lifecycle push failed: ${(err as Error).message}`),
      );
  };
  return {
    started: () => push(formatTaskStarted(opts.title)),
    completed: () => push(formatTaskCompleted(opts.title)),
    failed: (error: string) => push(formatTaskFailed(opts.title, error)),
  };
}

/**
 * Wrap a turn handler so every turn reports its lifecycle. The inner handler
 * calls the reporter at its start / terminal points; this wrapper only builds
 * the reporter (title comes from the inbound text) and stitches it into the
 * TurnContext.
 */
export function withTaskLifecycle(inner: TurnHandler): TurnHandler {
  return {
    handle(ctx: TurnContext): Promise<void> {
      const reporter = createTurnReporter({
        adapter: ctx.adapter,
        replyTarget: ctx.replyTarget,
        logger: ctx.logger,
        title: titleFromText(ctx.inbound.text),
      });
      return inner.handle({ ...ctx, reporter });
    },
  };
}
