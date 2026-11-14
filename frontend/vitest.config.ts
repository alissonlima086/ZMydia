import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  test: {
    environment: 'node',
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json-summary'],
      // Coverage is intentionally limited to framework-independent modules.
      // Svelte component behavior belongs in browser-level tests, not in this metric.
      include: ['src/lib/api.ts', 'src/lib/router.ts'],
      thresholds: {
        lines: 80,
        functions: 80,
        statements: 80,
        branches: 70
      }
    }
  }
});
