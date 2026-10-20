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

function isOutsideDirectory(directory: string, filePath: string): boolean {
  const relative = path.relative(directory, filePath);
  return relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative);
}

function generatedDocsAssetsPlugin() {
  return {
    name: 'generated-docs-assets',
    configureServer(server: import('vite').ViteDevServer) {
      server.middlewares.use('/docs/assets', (req, res, next) => {
        let requestPath: string;
        try {
          requestPath = decodeURIComponent((req.url ?? '').split('?')[0]);
        } catch {
          res.statusCode = 400;
          res.end('Invalid asset path');
          return;
        }
        if (requestPath.includes('\0') || requestPath.includes('\\')) {
          res.statusCode = 400;
          res.end('Invalid asset path');
          return;
        }
        const relativePath = requestPath.replace(/^\/+/, '');
        const filePath = path.resolve(generatedDocsAssetsDir, relativePath);
        if (isOutsideDirectory(generatedDocsAssetsDir, filePath)) {
          res.statusCode = 403;
          res.end('Asset path outside generated documentation');
          return;
        }
        if (!fs.existsSync(filePath) || !fs.statSync(filePath).isFile()) {
          next();
          return;
        }
        const canonicalRoot = fs.realpathSync(generatedDocsAssetsDir);
        const canonicalFile = fs.realpathSync(filePath);
        if (isOutsideDirectory(canonicalRoot, canonicalFile)) {
          res.statusCode = 403;
          res.end('Asset path outside generated documentation');
          return;
        }
        res.setHeader('Content-Type', contentTypeFor(canonicalFile));
        fs.createReadStream(canonicalFile).pipe(res);
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
    watch: {
      ignored: [
        '**/.dist/**',
        '**/dist/**',
        '**/target/**',
      ],
    },
  },
  build: {
    target: process.env.TAURI_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: !process.env.TAURI_DEBUG ? 'oxc' : false,
    sourcemap: Boolean(process.env.TAURI_DEBUG),
    rolldownOptions: {
      input: {
        app: resolve(__dirname, 'index.html'),
        eckyIr: resolve(__dirname, 'ecky-ir/index.html'),
      },
      output: {
        codeSplitting: {
          groups: [
            { name: 'three', test: /node_modules[\\/]three[\\/]/ },
            { name: 'editor', test: /node_modules[\\/](?:codemirror|@codemirror)[\\/]/ },
            { name: 'vendor', test: /node_modules[\\/](?:svelte|@tauri-apps)[\\/]/ },
          ],
        },
      },
    },
  },
});
