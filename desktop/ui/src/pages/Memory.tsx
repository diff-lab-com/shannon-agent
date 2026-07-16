// Memory page — thin wrapper around MemoryPanel.
//
// MemoryPanel is the real surface; this wrapper exists so react-router can
// lazy-load it as a page-level route (`/memory`).

import MemoryPanel from '@/components/memory/MemoryPanel'

export default function Memory() {
  return <MemoryPanel />
}
