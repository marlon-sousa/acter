import { defineConfig } from 'vitest/config';

export default defineConfig({
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  test: {
    environment: 'node',
  },
});
