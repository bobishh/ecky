import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [svelte()],
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    // Tauri supports es2021
    target: process.env.TAURI_PLATFORM == 'windows' ? 'chrome105' : 'safari13',
    // don't minify for debug builds
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    // produce sourcemaps for debug builds
    sourcemap: !!process.env.TAURI_DEBUG,
    rollupOptions: {
      output: {
        manualChunks: {
          three: ['three'],
          editor: ['codemirror', '@codemirror/state', '@codemirror/view', '@codemirror/language', '@codemirror/lang-python', '@codemirror/theme-one-dark'],
          vendor: ['svelte', '@tauri-apps/api', '@tauri-apps/plugin-dialog', '@tauri-apps/plugin-fs']
        }
      }
    }
  },
});

