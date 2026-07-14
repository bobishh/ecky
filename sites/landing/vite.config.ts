import { resolve } from 'node:path';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// The landing lives in sites/landing but reuses the Ecky genome verbatim from
// src/lib/genie (pure math: traits.ts + stoneGeometry.ts). Same genome ->
// same creature -> no drift between app and landing mascot.
const repoRoot = resolve(__dirname, '..', '..');

export default defineConfig({
  root: __dirname,
  plugins: [svelte()],
  resolve: {
    alias: {
      '@genome': resolve(repoRoot, 'src', 'lib', 'genie'),
    },
  },
  server: {
    port: 5174,
    strictPort: true,
    fs: { allow: [repoRoot] },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'safari13',
    chunkSizeWarningLimit: 800,
  },
});
