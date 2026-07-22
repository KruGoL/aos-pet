import { defineConfig } from 'vite'

export default defineConfig({
  base: './',            // built index.html is loaded via file://
  build: { outDir: 'dist' },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
} as any)
