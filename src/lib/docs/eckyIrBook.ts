import { parseDocsDocument, renderMarkdownFragment } from './eckyIrGuide';

export type EckyIrBookInput = {
  docsMarkdown: string;
  assetSourceRoot?: string;
};

export type EckyIrBookChapter = {
  id: string;
  title: string;
  bodyHtml: string;
};

export type EckyIrBookAsset = {
  sourcePath: string;
  outputPath: string;
  mediaType: string;
};

export type EckyIrBook = {
  title: string;
  summaryHtml: string;
  chapters: EckyIrBookChapter[];
  assets: EckyIrBookAsset[];
  html: string;
};

export function buildEckyIrBook(input: EckyIrBookInput): EckyIrBook {
  const docs = parseDocsDocument(input.docsMarkdown);
  const title = 'Ecky IR Field Guide';
  const assets = collectBookAssets(
    input.docsMarkdown,
    input.assetSourceRoot ?? 'target/book/public/docs',
  );
  const chapters: EckyIrBookChapter[] = [
    ...docs.sections.map((section) => ({
      id: `docs-${section.slug}`,
      title: section.title,
      bodyHtml: section.bodyHtml,
    })),
  ];

  const tocHtml = chapters
    .map((chapter) => `<li><a href="#${chapter.id}">${escapeHtml(chapter.title)}</a></li>`)
    .join('');
  const chaptersHtml = chapters
    .map(
      (chapter) => `
        <section class="chapter" id="${chapter.id}">
          <h2>${escapeHtml(chapter.title)}</h2>
          ${chapter.bodyHtml}
        </section>`,
    )
    .join('');

  const html = `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>${escapeHtml(title)}</title>
    <style>
      html {
        font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", Palatino, serif;
        color: #1f1a15;
        background: #f6f0e4;
      }

      body {
        margin: 0 auto;
        max-width: 52rem;
        padding: 2.5rem 1.75rem 4rem;
        line-height: 1.6;
      }

      .cover,
      .chapter,
      .toc {
        page-break-after: always;
      }

      .cover {
        min-height: 80vh;
        display: flex;
        flex-direction: column;
        justify-content: center;
        border: 3px solid #8b6a2b;
        padding: 2.5rem;
        background:
          linear-gradient(180deg, rgba(139, 106, 43, 0.10), rgba(139, 106, 43, 0.02)),
          #f8f2e8;
      }

      .cover .kicker,
      .toc h2 {
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

      p,
      li {
        font-size: 1rem;
      }

      ul {
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

      .summary p {
        font-size: 1.08rem;
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
      }
    </style>
  </head>
  <body>
    <section class="cover">
      <p class="kicker">Ecky CAD / language reference</p>
      <h1>${escapeHtml(title)}</h1>
      <div class="summary">
        ${docs.summaryHtml}
        <p>Lessons move from one primitive solid to a complete multi-part model.</p>
      </div>
    </section>
    <nav class="toc" id="toc">
      <h2>Table of Contents</h2>
      <ol>${tocHtml}</ol>
    </nav>
    ${chaptersHtml}
  </body>
</html>`;

  return {
    title,
    summaryHtml: docs.summaryHtml,
    chapters,
    assets,
    html,
  };
}

function collectBookAssets(docsMarkdown: string, assetSourceRoot: string): EckyIrBookAsset[] {
  const assets = new Map<string, EckyIrBookAsset>();

  for (const matched of docsMarkdown.matchAll(/!\[[^\]]*\]\(([^)]+)\)/g)) {
    const outputPath = matched[1]?.trim();
    if (!outputPath) continue;
    registerAsset(assets, assetSourceRoot, outputPath);
  }

  return [...assets.values()];
}

function registerAsset(store: Map<string, EckyIrBookAsset>, assetSourceRoot: string, outputPath: string) {
  if (store.has(outputPath)) return;

  store.set(outputPath, {
    sourcePath: joinAssetPath(assetSourceRoot, outputPath),
    outputPath,
    mediaType: mediaTypeFor(outputPath),
  });
}

function joinAssetPath(root: string, outputPath: string): string {
  return `${root.replace(/\/+$/, '')}/${outputPath.replace(/^\/+/, '')}`;
}

function mediaTypeFor(outputPath: string): string {
  if (outputPath.endsWith('.svg')) return 'image/svg+xml';
  if (outputPath.endsWith('.png')) return 'image/png';
  if (outputPath.endsWith('.jpg') || outputPath.endsWith('.jpeg')) return 'image/jpeg';
  return 'application/octet-stream';
}

function escapeHtml(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');
}
