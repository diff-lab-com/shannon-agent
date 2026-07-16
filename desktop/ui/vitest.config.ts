import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./src/__tests__/setup.ts'],
    poolOptions: {
      threads: {
        maxThreads: 1,
        minThreads: 1,
      },
      forks: {
        maxForks: 1,
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
        'src/hooks/useTheme.ts',
        'src/components/ui/select.tsx',
        'src/components/ui/tabs.tsx',
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
