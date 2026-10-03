import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import os from 'node:os'
import path from 'path'
import pkg from './package.json' with { type: 'json' }

// Parallel workers = half the machine's CPUs (min 2). Half leaves headroom
// for the editor/TS server on dev machines and for sibling CI jobs; the
// floor keeps 4-vCPU CI runners at 2 workers. The default pool is `forks`
// (vitest 2.x), so `maxForks` is the binding limit — `maxThreads` is kept
// in sync for anyone switching pools.
const workers = Math.max(2, Math.floor(os.availableParallelism() / 2))

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Mirror vite.config.ts's `__APP_VERSION__` define: the mock-handler
  // coverage tripwire imports the mock layer (handlers.ts → data/config.ts),
  // which references that global — undefined under vitest otherwise, because
  // this file is separate from vite.config.ts and its define doesn't apply.
  define: {
    '__APP_VERSION__': JSON.stringify(pkg.version),
  },
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./src/__tests__/setup.ts'],
    poolOptions: {
      threads: {
        maxThreads: workers,
        minThreads: 1,
      },
      forks: {
        maxForks: workers,
        minForks: 1,
      },
    },
    maxConcurrency: 1,
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json', 'html', 'lcov'],
      include: ['src/**/*.tsx', 'src/**/*.ts'],
      exclude: [
        'node_modules/',
        'src/__tests__/',
        '**/*.test.{ts,tsx}',
        '**/*.spec.{ts,tsx}',
        'src/main.tsx',
        'src/vite-env.d.ts',
        'src/types/index.ts',
        'src/lib/tauri-api.ts',
        'src/App.tsx',
        // Base UI 1.8's Tooltip drives open/close with dense internal timers;
        // under V8 coverage instrumentation those callbacks are amplified ~1000x
        // (6 tests took 15 minutes). The shim is 12 lines of composition — its
        // behaviour is covered by Tooltip.test + e2e, not by coverage percentages.
        'src/components/ui/tooltip.prim.tsx',
        // Demo-mode mock layer (VITE_MOCK_MODE=1): only reachable via the
        // main.tsx alias swap, never in production builds — same rationale
        // as main.tsx/App.tsx above. 1864 lines of mock data/handlers were
        // dragging real coverage ~6pp below the threshold.
        'src/lib/mock/',
        'src/hooks/useTheme.ts',
        'src/components/ui/select.tsx',
      ],
      thresholds: {
        lines: 80,
        functions: 60,
        branches: 75,
        statements: 80
      }
    },
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
    root: path.resolve(__dirname)
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src')
    }
  }
})
