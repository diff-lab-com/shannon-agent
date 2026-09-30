/**
 * R2-1: promote a model target to the engine-global default — exactly the
 * configure('model') + configure('provider') pair the chip performed before
 * R2-1, followed by the config/status refreshes the header reads.
 *
 * Extracted as a pure async function so the promotion contract is unit-test
 * able without driving the Base UI popup through jsdom (known-slow there —
 * see ChatInput.test.tsx).
 */
export interface PromotionTarget {
  /** Canonical catalog model id (e.g. `anthropic-claude-sonnet-4-6`). */
  id: string
  /** Provider kind slug (e.g. `anthropic`). */
  provider: string
}

export interface PromotionHooks {
  configure: (args: { key: string; value: string }) => Promise<unknown>
  refreshConfig: () => Promise<unknown> | void
  refreshStatus: () => Promise<unknown> | void
}

export async function promoteSessionModelToDefault(
  target: PromotionTarget,
  hooks: PromotionHooks,
): Promise<void> {
  await hooks.configure({ key: 'model', value: target.id })
  await hooks.configure({ key: 'provider', value: target.provider })
  await hooks.refreshConfig()
  await hooks.refreshStatus()
}
