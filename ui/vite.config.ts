import { fileURLToPath } from 'node:url';

import { defineConfig } from 'vitest/config';

export default defineConfig({
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    rollupOptions: {
      input: fileURLToPath(new URL('src/views/main_window.html', import.meta.url)),
    },
  },
  test: {
    environment: 'node',
  },
});
