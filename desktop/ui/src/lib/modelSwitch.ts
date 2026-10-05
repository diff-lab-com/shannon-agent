// S3-1 (P-N23) — the ONE write path for "change the global default model".
//
// The review flagged the write-convention fork: Header wrote the
// `model`+`provider` key pair while the Settings quick switcher wrote only
// `model` — harmless today (the catalog comes from the active provider) but
// a foot-gun the moment the catalog's data source changes. Both now funnel
// through this helper, exactly like the composer chip's pre-R2-1 global
// branch did.
//
// `configure('model')` normalizes legacy display names to catalog ids
// backend-side (`normalize_model_id`); the provider write mirrors the same
// active slot the catalog row came from, so the pair stays consistent.

import * as api from '@/lib/tauri-api'
import type { ModelInfo } from '@/types'

/** The catalog row shape the helper needs (ModelInfo subset). */
export type GlobalModelTarget = Pick<ModelInfo, 'id' | 'provider'>

/**
 * Write the global default as the `model`+`provider` key pair (P-N23).
 * Throws on the first failed configure — callers own toasts/refreshes.
 */
export async function writeGlobalModelDefault(model: GlobalModelTarget): Promise<void> {
  await api.configure({ key: 'model', value: model.id })
  await api.configure({ key: 'provider', value: model.provider })
}
