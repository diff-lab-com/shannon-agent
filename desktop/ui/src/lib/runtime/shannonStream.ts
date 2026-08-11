/**
 * P2-5a expansion — narrow indirection between the assistant-ui runtime
 * adapter and the real Tauri bridge.
 *
 * Why split this out:
 *   • `chatModelAdapter.ts` owns message-part translation (assistant-ui
 *     shapes). Keeping it Tauri-free means it can be unit-tested without
 *     touching the global `@tauri-apps/api/event` mock.
 *   • `tauriBridge.ts` owns event subscription + `invoke('send_message')`.
 *     It is the only module that imports `@tauri-apps/api/*` directly.
 *   • `shannonStream.ts` is the typed seam they share: just the union
 *     of streaming event shapes and the bridge interface.
 *
 * This file must stay minimal — adding imports here will defeat the
 * separation of concerns above.
 */
export type {
  SendMessageResponse,
  ShannonStreamEvent,
  ShannonStreamHandler,
  ShannonTauriBridge,
} from './tauriBridge';

export { RealShannonTauriBridge } from './tauriBridge';