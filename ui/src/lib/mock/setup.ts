// Mock mode helpers (runtime).
//
// Mock mode is enabled via Vite alias at build time (see vite.config.ts) when
// VITE_MOCK_MODE is set. At runtime we expose:
//   - isMockMode(): check from anywhere (e.g. components that want to know)
//   - setupMockMode(): no-op kept for backwards compat; mock is wired at module load

export function isMockMode(): boolean {
  if (typeof window === 'undefined') return false
  const url = new URL(window.location.href)
  if (url.searchParams.get('demo') === '1') return true
  if (localStorage.getItem('shannon:mock') === '1') return true
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const env = (import.meta as any).env
  return env?.VITE_MOCK_MODE === '1' || env?.VITE_MOCK_MODE === 'true'
}

// Mock is wired by the Vite alias + coreMock.ts at module load. This function
// is a no-op kept so main.tsx doesn't need to know the implementation detail.
export function setupMockMode(): void {
  // intentionally empty — see src/lib/mock/README.md
}
