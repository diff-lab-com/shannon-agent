/**
 * P2-5a — chat.v2 runtime provider.
 *
 * Wraps children in `<AssistantRuntimeProvider>` driven by
 * `useExternalStoreRuntime(makeShannonTauriAdapter(...))`. The
 * adapter is created once via `useRef` so the runtime doesn't
 * get rebuilt on every render.
 *
 * The provider is a no-op (returns children verbatim) when the
 * `chat.v2` flag is OFF — keeping the legacy `Chat.tsx` rendering
 * path byte-equivalent to the pre-rollout behaviour. Acceptance
 * criterion #3 in `docs/plans/chat-upgrade.md §3.1`.
 */
import { useRef, type ReactNode } from 'react'
import {
  AssistantRuntimeProvider,
  useExternalStoreRuntime,
  type ThreadMessage,
} from '@assistant-ui/react'
import { makeShannonTauriAdapter } from './shannonTauriRuntime'
import { RealShannonTauriBridge } from './tauriBridge'
import { isChatV2Enabled } from '@/lib/featureFlag'

export interface ChatV2RuntimeProviderProps {
  children: ReactNode
  /**
   * Override the bridge — tests pass a mock; production uses the
   * real Tauri bridge. Defaults to a shared `RealShannonTauriBridge`
   * instance so we don't spin up multiple listener pools.
   */
  bridgeFactory?: () => RealShannonTauriBridge
  /**
   * Optional initial message list. Default = empty; production
   * rollout replaces this with `ChatContext`'s hydrated transcript
   * (P2-5a ships empty so the feature flag is observable but no
   * data flow is forced).
   */
  initialMessages?: readonly ThreadMessage[]
}

export function ChatV2RuntimeProvider({
  children,
  bridgeFactory,
  initialMessages,
}: ChatV2RuntimeProviderProps) {
  const enabled = isChatV2Enabled()
  if (!enabled) {
    // Legacy path — no provider, no runtime hook, no behaviour
    // change. Children render verbatim.
    return <>{children}</>
  }

  return (
    <ChatV2RuntimeEnabled bridgeFactory={bridgeFactory} initialMessages={initialMessages}>
      {children}
    </ChatV2RuntimeEnabled>
  )
}

interface EnabledProps {
  children: ReactNode
  bridgeFactory?: () => RealShannonTauriBridge
  initialMessages?: readonly ThreadMessage[]
}

/** Inner component — only mounted when chat.v2 is enabled. */
function ChatV2RuntimeEnabled({
  children,
  bridgeFactory,
  initialMessages,
}: EnabledProps) {
  // Lazy-init the adapter so it survives across renders without
  // being recreated when the parent passes a fresh function
  // identity. Inputs are immutable for the page's lifetime, so a
  // single bridge per provider is correct.
  const adapterRef = useRef<ReturnType<typeof makeShannonTauriAdapter> | null>(null)
  if (adapterRef.current === null) {
    const bridge = bridgeFactory ? bridgeFactory() : new RealShannonTauriBridge()
    adapterRef.current = makeShannonTauriAdapter({ bridge, initialMessages })
  }
  // `useExternalStoreRuntime` is the hook that returns an
  // `AssistantRuntime` — calling it every render is required;
  // the hook subscribes to the adapter's mutable state.
  const runtime = useExternalStoreRuntime(adapterRef.current)
  return <AssistantRuntimeProvider runtime={runtime}>{children}</AssistantRuntimeProvider>
}
