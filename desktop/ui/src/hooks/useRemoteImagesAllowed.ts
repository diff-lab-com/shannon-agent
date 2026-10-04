import { useSyncExternalStore } from 'react'
import { isRemoteImagesAllowed, subscribeRemoteImagesAllowed } from '@/lib/remoteImages'

/**
 * P2-4: the global "allow remote images" switch as a hook. Deliberately a
 * store subscription, NOT context: the only consumer is `LocalImage`, which
 * sits inside memoized Markdown subtrees where threading a new context prop
 * would re-render every message on a toggle. `useSyncExternalStore` wakes
 * exactly the mounted gate sites when `setRemoteImagesAllowed` fires.
 */
export function useRemoteImagesAllowed(): boolean {
  return useSyncExternalStore(subscribeRemoteImagesAllowed, isRemoteImagesAllowed)
}
