import path from 'node:path';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Tauri loads from src/dist/ via tauri.conf.json frontendDist = "../src/dist".
// We write the build there so the existing Tauri build pipeline keeps
// shipping the bundled .app without extra wiring.
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, './src'),
    },
  },
  clearScreen: false,
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'es2022',
    sourcemap: false,
  },
  server: {
    port: 1420,
    strictPort: true,
    host: '127.0.0.1',
  },
});
