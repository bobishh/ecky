import fs from 'node:fs';
import path, { resolve } from 'node:path';
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const generatedDocsAssetsDir = resolve(__dirname, 'target', 'book', 'public', 'docs', 'assets');
const generatedDocsBooksDir = resolve(__dirname, 'target', 'book', 'dist', 'books');
const generatedDocsEpubFile = resolve(generatedDocsBooksDir, 'ecky-ir-field-guide.epub');

function contentTypeFor(filePath: string): string {
  if (filePath.endsWith('.png')) return 'image/png';
  if (filePath.endsWith('.svg')) return 'image/svg+xml';
  if (filePath.endsWith('.jpg') || filePath.endsWith('.jpeg')) return 'image/jpeg';
  if (filePath.endsWith('.webp')) return 'image/webp';
  if (filePath.endsWith('.epub')) return 'application/epub+zip';
  return 'application/octet-stream';
}

function generatedDocsAssetsPlugin() {
  return {
    name: 'generated-docs-assets',
    configureServer(server: import('vite').ViteDevServer) {
      server.middlewares.use('/docs/assets', (req, res, next) => {
        const requestPath = (req.url ?? '').split('?')[0];
        const relativePath = requestPath.replace(/^\/+/, '');
        const filePath = path.join(generatedDocsAssetsDir, relativePath);
        if (!fs.existsSync(filePath) || !fs.statSync(filePath).isFile()) {
          next();
          return;
        }
        res.setHeader('Content-Type', contentTypeFor(filePath));
        fs.createReadStream(filePath).pipe(res);
      });
      server.middlewares.use('/docs/ecky-ir-field-guide.epub', (_req, res, next) => {
        if (!fs.existsSync(generatedDocsEpubFile) || !fs.statSync(generatedDocsEpubFile).isFile()) {
          next();
          return;
        }
        res.setHeader('Content-Type', contentTypeFor(generatedDocsEpubFile));
        fs.createReadStream(generatedDocsEpubFile).pipe(res);
      });
    },
    writeBundle() {
      if (!fs.existsSync(generatedDocsAssetsDir)) {
        if (fs.existsSync(generatedDocsEpubFile)) {
          const distDocsDir = resolve(__dirname, 'dist', 'docs');
          fs.mkdirSync(distDocsDir, { recursive: true });
          fs.copyFileSync(generatedDocsEpubFile, path.join(distDocsDir, 'ecky-ir-field-guide.epub'));
        }
        return;
      }
      const distDocsAssetsDir = resolve(__dirname, 'dist', 'docs', 'assets');
      fs.mkdirSync(path.dirname(distDocsAssetsDir), { recursive: true });
      fs.cpSync(generatedDocsAssetsDir, distDocsAssetsDir, { recursive: true });
      if (fs.existsSync(generatedDocsEpubFile)) {
        fs.copyFileSync(
          generatedDocsEpubFile,
          resolve(__dirname, 'dist', 'docs', 'ecky-ir-field-guide.epub'),
        );
      }
    },
  };
}

export default defineConfig({
  plugins: [svelte(), generatedDocsAssetsPlugin()],
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    target: process.env.TAURI_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: Boolean(process.env.TAURI_DEBUG),
    rollupOptions: {
      input: {
        app: resolve(__dirname, 'index.html'),
        eckyIr: resolve(__dirname, 'ecky-ir/index.html'),
      },
      output: {
        manualChunks: {
          three: ['three'],
          editor: [
            'codemirror',
            '@codemirror/state',
            '@codemirror/view',
            '@codemirror/language',
            '@codemirror/lang-python',
            '@codemirror/theme-one-dark',
          ],
          vendor: [
            'svelte',
            '@tauri-apps/api',
            '@tauri-apps/plugin-dialog',
            '@tauri-apps/plugin-fs',
          ],
        },
      },
    },
  },
});
