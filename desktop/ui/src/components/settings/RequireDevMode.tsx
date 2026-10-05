import { type ReactNode } from 'react'
import { Navigate, Outlet } from 'react-router-dom'
import { useSidebarMode } from '@/components/Sidebar'

/**
 * Route guard for /settings/advanced (Settings R3, T1): the Advanced
 * section stays dev-gated now that the in-page rail is the only section
 * nav — deep links from a simple-mode session bounce to General instead
 * of rendering the developer surface.
 */
export default function RequireDevMode({ children }: { children?: ReactNode }) {
  const [mode] = useSidebarMode()
  if (mode !== 'dev') {
    return <Navigate to="/settings/general" replace />
  }
  return children ?? <Outlet />
}
