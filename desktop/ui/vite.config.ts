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
      '@': '/src',
      // Swap the Tauri core module with our mock when demo mode is on.
      // This is the only way to intercept invoke() calls cleanly in ESM.
      ...(mockMode ? { '@tauri-apps/api/core': path.resolve(__dirname, 'src/lib/mock/coreMock.ts') } : {}),
    }
  },
  build: {
    target: 'es2020',
    outDir: 'dist'
  },
  server: {
    port: 1420
  }
})
