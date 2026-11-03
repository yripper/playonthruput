import { defineConfig } from 'vite';

export default defineConfig({
  // main.ts awaits the wasm engine at the top level.
  build: { target: 'es2022' },
});
