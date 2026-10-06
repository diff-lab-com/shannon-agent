import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'
import path from 'node:path'
import pkg from './package.json' with { type: 'json' }

const mockMode = process.env.VITE_MOCK_MODE === '1' || process.env.VITE_MOCK_MODE === 'true'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  define: {
    'import.meta.env.VITE_MOCK_MODE': JSON.stringify(mockMode ? '1' : '0'),
    '__APP_VERSION__': JSON.stringify(pkg.version),
  },
  resolve: {
    alias: {
      // Must be a real absolute path: esbuild's dev-server dependency
      // scanner stats the aliased directory to resolve `/index.ts`, and a
      // root-relative '/src' doesn't exist on disk — runtime-value imports
      // like `import { EVENT_NAMES } from '@/types'` then fail the whole
      // dev-server build (production build and tsc are unaffected, which
      // is why only `pnpm demo` / Desktop E2E went red).
      '@': path.resolve(__dirname, 'src'),
      // Swap the Tauri core module with our mock when demo mode is on.
      // This is the only way to intercept invoke() calls cleanly in ESM.
      ...(mockMode ? { '@tauri-apps/api/core': path.resolve(__dirname, 'src/lib/mock/coreMock.ts') } : {}),
    }
  },
  optimizeDeps: {
    // Mock mode only: the dep optimizer APPLIES resolve.alias while
    // pre-bundling, so it folds the aliased coreMock.ts INTO the
    // @tauri-apps/plugin-dialog chunk — a second module instance of the
    // whole mock layer, whose late module init re-runs the scripted
    // backend's boot (`loadScript`) and wipes player state mid-session.
    // (Measured in vite 6.4.3: every demo page evaluated coreMock twice —
    // once from /src, once from node_modules/.vite/deps.) Excluding the
    // plugin serves it as source, so its `@tauri-apps/api/core` import
    // resolves through the same alias — one coreMock instance per page.
    ...(mockMode ? { exclude: ['@tauri-apps/plugin-dialog'] } : {}),
  },
  build: {
    target: 'es2020',
    outDir: 'dist'
  },
  server: {
    port: 1420
  }
})
