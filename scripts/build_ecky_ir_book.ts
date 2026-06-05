import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

import { buildEckyIrBook } from '../src/lib/docs/eckyIrBook';

const root = process.cwd();
const bookTargetDir = path.join(root, 'target', 'book');
const outputDir = path.join(bookTargetDir, 'dist', 'books');
const htmlPath = path.join(outputDir, 'ecky-ir-field-guide.html');
const epubPath = path.join(outputDir, 'ecky-ir-field-guide.epub');
const epubWorkDir = path.join(outputDir, 'ecky-ir-field-guide-epub');
const docsSourcePath = path.join(root, 'public', 'docs', 'ecky-ir.md');
// The app serves the reader markdown, the EPUB download, and chapter images
// from `public/docs` (Vite static root). Publish the freshly built artifacts
// there so a single `build:book` keeps both the in-app reader and the EPUB
// download current — no manual second copy.
const publicDocsDir = path.join(root, 'public', 'docs');

const docsMarkdown = fs.readFileSync(docsSourcePath, 'utf8');

const book = buildEckyIrBook({
  docsMarkdown,
  assetSourceRoot: 'target/book/public/docs',
});

fs.mkdirSync(outputDir, { recursive: true });
fs.writeFileSync(htmlPath, book.html);
copyHtmlAssets(book);
writeEpub(book);
publishToPublicDocs(book);

console.log(`HTML: ${htmlPath}`);
console.log(`EPUB: ${epubPath}`);
console.log(`Published EPUB/HTML + assets to: ${publicDocsDir}`);

function writeEpub(book: ReturnType<typeof buildEckyIrBook>) {
  const metaInfDir = path.join(epubWorkDir, 'META-INF');
  const oebpsDir = path.join(epubWorkDir, 'OEBPS');

  fs.rmSync(epubWorkDir, { recursive: true, force: true });
  fs.mkdirSync(metaInfDir, { recursive: true });
  fs.mkdirSync(oebpsDir, { recursive: true });

  fs.writeFileSync(path.join(epubWorkDir, 'mimetype'), 'application/epub+zip');
  fs.writeFileSync(
    path.join(metaInfDir, 'container.xml'),
    `<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/package.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>`,
  );
  fs.writeFileSync(path.join(oebpsDir, 'styles.css'), bookStyles());
  fs.writeFileSync(path.join(oebpsDir, 'content.xhtml'), bookContentXhtml(book));
  fs.writeFileSync(path.join(oebpsDir, 'nav.xhtml'), bookNavXhtml(book));
  fs.writeFileSync(path.join(oebpsDir, 'package.opf'), bookPackageOpf(book));
  copyEpubAssets(book, oebpsDir);

  fs.rmSync(epubPath, { force: true });
  execFileSync('zip', ['-X0', epubPath, 'mimetype'], {
    cwd: epubWorkDir,
    stdio: 'inherit',
  });
  execFileSync('zip', ['-Xur9D', epubPath, 'META-INF', 'OEBPS'], {
    cwd: epubWorkDir,
    stdio: 'inherit',
  });
}

function bookContentXhtml(book: ReturnType<typeof buildEckyIrBook>): string {
  const chapters = book.chapters
    .map(
      (chapter) => `
    <section class="chapter" id="${escapeXml(chapter.id)}">
      <h2>${escapeXml(chapter.title)}</h2>
      ${chapter.bodyHtml}
    </section>`,
    )
    .join('');

  return `<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="en" xml:lang="en">
  <head>
    <title>${escapeXml(book.title)}</title>
    <link rel="stylesheet" type="text/css" href="styles.css" />
  </head>
  <body>
    <section class="cover">
      <p class="kicker">Ecky CAD / language reference</p>
      <h1>${escapeXml(book.title)}</h1>
      <div class="summary">
        ${book.summaryHtml}
        <p>This edition packages the canonical Ecky IR language reference as a single EPUB.</p>
      </div>
    </section>
    ${chapters}
  </body>
</html>`;
}

function bookNavXhtml(book: ReturnType<typeof buildEckyIrBook>): string {
  const items = book.chapters
    .map(
      (chapter) => `<li><a href="content.xhtml#${escapeXml(chapter.id)}">${escapeXml(chapter.title)}</a></li>`,
    )
    .join('');

  return `<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="en" xml:lang="en">
  <head>
    <title>Table of Contents</title>
  </head>
  <body>
    <nav epub:type="toc" id="toc">
      <h1>Table of Contents</h1>
      <ol>${items}</ol>
    </nav>
  </body>
</html>`;
}

function bookPackageOpf(book: ReturnType<typeof buildEckyIrBook>): string {
  const assetItems = book.assets
    .map(
      (asset, index) =>
        `<item id="asset-${index}" href="${escapeXml(asset.outputPath)}" media-type="${escapeXml(asset.mediaType)}" />`,
    )
    .join('\n    ');

  return `<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" unique-identifier="bookid" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:ecky:ir-field-guide</dc:identifier>
    <dc:title>${escapeXml(book.title)}</dc:title>
    <dc:language>en</dc:language>
    <dc:creator>OpenAI Codex</dc:creator>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav" />
    <item id="content" href="content.xhtml" media-type="application/xhtml+xml" />
    <item id="styles" href="styles.css" media-type="text/css" />
    ${assetItems}
  </manifest>
  <spine>
    <itemref idref="nav" linear="no" />
    <itemref idref="content" />
  </spine>
</package>`;
}

function bookStyles(): string {
  return `html {
  font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Palatino, serif;
  color: #1f1a15;
  background: #f6f0e4;
}

body {
  margin: 0 auto;
  max-width: 52rem;
  line-height: 1.6;
}

.cover,
.chapter {
  page-break-after: always;
}

.cover {
  min-height: 80vh;
  border: 3px solid #8b6a2b;
  padding: 2.5rem;
  background:
    linear-gradient(180deg, rgba(139, 106, 43, 0.10), rgba(139, 106, 43, 0.02)),
    #f8f2e8;
}

.chapter {
  padding: 1.4rem 0;
}

.kicker {
  text-transform: uppercase;
  letter-spacing: 0.12em;
  font-size: 0.8rem;
  color: #8b6a2b;
  margin: 0 0 0.75rem;
}

h1,
h2,
h3,
h4 {
  font-family: "Avenir Next Condensed", "Arial Narrow", sans-serif;
  line-height: 1.1;
  color: #20150b;
}

h1 {
  font-size: 2.8rem;
  margin: 0;
}

h2 {
  font-size: 1.9rem;
  margin: 0 0 1rem;
  padding-bottom: 0.4rem;
  border-bottom: 2px solid #c3a25f;
}

h3 {
  font-size: 1.25rem;
  margin-top: 1.8rem;
}

h4 {
  font-size: 1rem;
  margin-top: 1.4rem;
}

ul,
ol {
  padding-left: 1.25rem;
}

code,
pre {
  font-family: "SF Mono", "Fira Code", "Cascadia Code", monospace;
}

code {
  background: rgba(139, 106, 43, 0.10);
  padding: 0.08rem 0.22rem;
}

pre {
  overflow-x: auto;
  background: #1b1a18;
  color: #efe5d2;
  padding: 1rem;
  border: 1px solid #8b6a2b;
  line-height: 1.45;
  white-space: pre-wrap;
}

a {
  color: #6b4c18;
  text-decoration: none;
}

figure {
  margin: 1.25rem 0 1.5rem;
}

figure img {
  display: block;
  width: 100%;
  height: auto;
  border: 1px solid #8b6a2b;
  background: #f8f2e8;
}

figcaption {
  margin-top: 0.45rem;
  font-size: 0.92rem;
  color: #5a4731;
}`;
}

// Publish the built artifacts into the Vite static root so the in-app reader
// (markdown + images) and the EPUB download both serve fresh content from one
// build. The reader markdown itself is the source (`public/docs/ecky-ir.md`)
// and is left untouched; only the generated EPUB/HTML and chapter images are
// copied here.
function publishToPublicDocs(book: ReturnType<typeof buildEckyIrBook>) {
  fs.mkdirSync(publicDocsDir, { recursive: true });
  fs.copyFileSync(epubPath, path.join(publicDocsDir, 'ecky-ir-field-guide.epub'));
  fs.copyFileSync(htmlPath, path.join(publicDocsDir, 'ecky-ir-field-guide.html'));
  for (const asset of book.assets) {
    const source = path.join(root, asset.sourcePath);
    const target = path.join(publicDocsDir, asset.outputPath);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.copyFileSync(source, target);
  }
}

function copyHtmlAssets(book: ReturnType<typeof buildEckyIrBook>) {
  for (const asset of book.assets) {
    const source = path.join(root, asset.sourcePath);
    const target = path.join(outputDir, asset.outputPath);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.copyFileSync(source, target);
  }
}

function copyEpubAssets(book: ReturnType<typeof buildEckyIrBook>, oebpsDir: string) {
  for (const asset of book.assets) {
    const source = path.join(root, asset.sourcePath);
    const target = path.join(oebpsDir, asset.outputPath);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.copyFileSync(source, target);
  }
}

function escapeXml(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&apos;');
}
