import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// The WASM bindings are generated into `src/wasm` by `npm run build:wasm`.
// Vite understands the `new URL('nes_wasm_bg.wasm', import.meta.url)` pattern
// the generated bindings use, so the binary is copied to the output for free.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    host: true,
  },
  build: {
    target: 'es2022',
    outDir: 'dist',
  },
});
