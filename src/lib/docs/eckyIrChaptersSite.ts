import { docsHeadingSlug, renderMarkdownFragment } from './eckyIrGuide';

export type StaticChapterCheckpoint = {
  id: string;
  source: string;
  code: string;
  assetPath?: string;
};

export type StaticChapter = {
  id: string;
  sectionSlug: string;
  title: string;
  markdown: string;
  checkpoints: StaticChapterCheckpoint[];
};

export type ChaptersSiteOptions = {
  basePath: string;
  epubPath: string;
};

export function buildChaptersSitePages(
  chapters: StaticChapter[],
  options: ChaptersSiteOptions,
): Map<string, string> {
  const pages = new Map<string, string>();
  pages.set('chapters/index.html', buildChaptersIndexHtml(chapters, options));
  for (const chapter of chapters) {
    pages.set(
      `chapters/${chapter.sectionSlug}/index.html`,
      buildChapterHtml(chapter, chapters, options),
    );
  }
  return pages;
}

export function buildChaptersIndexHtml(
  chapters: StaticChapter[],
  options: ChaptersSiteOptions,
): string {
  const cards = chapters.map((chapter, index) => `<a class="docs-pager__link" href="${chapterRoute(chapter, options)}">
    <span>Chapter ${String(index + 1).padStart(2, '0')}</span>
    <strong>${escapeHtml(chapter.title)}</strong>
  </a>`).join('');
  return documentHtml('Chapters · Ecky CAD', options, `
    <main class="docs-main docs-main--chapters">
      <section class="docs-main__section">
        <p class="docs-main__eyebrow">ECKY CAD / CHAPTERS</p>
        <h1 class="docs-main__heading">Six practical chapters</h1>
        <div class="docs-main__body"><p>Start with two boxes and a union. Then work through fits, lofts, a travel case, a phone case, and a film scanner. Each chapter includes code to read and complete .ecky files to open in Ecky. The later chapters are source walkthroughs; begin with chapter 1 if the language is new to you.</p></div>
        <nav class="docs-pager docs-pager--chapters" aria-label="Chapters">${cards}</nav>
      </section>
    </main>`);
}

function buildChapterHtml(
  chapter: StaticChapter,
  chapters: StaticChapter[],
  options: ChaptersSiteOptions,
): string {
  const index = chapters.findIndex((candidate) => candidate.id === chapter.id);
  const previous = index > 0 ? chapters[index - 1] : null;
  const next = index < chapters.length - 1 ? chapters[index + 1] : null;
  const sourceBlocks = uniqueSources(chapter.checkpoints).map((checkpoint) => renderSource(checkpoint)).join('');
  return documentHtml(`${escapeHtml(chapter.title)} · Ecky chapters`, options, `
    <main class="docs-main docs-main--chapters">
      <section class="docs-main__section">
        <p class="docs-main__eyebrow">CHAPTER ${String(index + 1).padStart(2, '0')} / 06</p>
        <h1 class="docs-main__heading">${escapeHtml(chapter.title)}</h1>
        <div class="docs-main__body">${renderChapterContents(chapter.markdown)}${renderChapterMarkdown(chapter.markdown)}</div>
        ${sourceBlocks ? `<section class="docs-chapter-sources" aria-label="Ecky sources"><h2>Complete examples</h2>${sourceBlocks}</section>` : ''}
      </section>
      ${renderPager(previous, next, options)}
    </main>`);
}

function documentHtml(title: string, options: ChaptersSiteOptions, main: string): string {
  const base = normalizeBasePath(options.basePath);
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>${title}</title>
    <meta name="description" content="Learn Ecky CAD with explained examples and downloadable model source." />
    <link rel="icon" href="/favicon.svg" type="image/svg+xml" />
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link href="https://fonts.googleapis.com/css2?family=Fira+Code:wght@400;500;600&family=Space+Grotesk:wght@500;600;700&display=swap" rel="stylesheet" />
    <style>${renderChapterStylesheet()}</style>
  </head>
  <body>
    <header class="docs-header"><div class="docs-header__inner">
      <a class="docs-header__home" href="/">← Ecky CAD</a><span class="docs-header__sep">/</span>
      <a class="docs-header__title" href="${base}/chapters/">Chapters</a>
      <nav class="docs-header__actions" aria-label="Documentation">
        <a class="docs-action" href="${base}/chapters/">Chapters</a>
        <a class="docs-action" href="${base}/">Reference</a>
        <a class="docs-action docs-action--primary" href="${escapeHtml(options.epubPath)}">EPUB</a>
      </nav>
    </div></header>
    <div class="docs-shell"><div class="docs-layout docs-layout--chapters">${main}</div></div>
    <footer class="docs-footer"><span>Ecky CAD</span><a href="${base}/">Function reference</a></footer>
  </body>
</html>`;
}

function renderChapterMarkdown(markdown: string): string {
  const withoutFrontMatter = markdown.replace(/^---\s*\n[\s\S]*?\n---\s*\n/, '');
  const withoutTitle = withoutFrontMatter.replace(/^#\s+.+\n+/, '');
  const normalizedHeadings = withoutTitle
    .replace(/^###\s+/gm, '#### ')
    .replace(/^##\s+/gm, '### ');
  return renderMarkdownFragment(normalizedHeadings).replace(/<h([34]) id="[^"]*">(.*?) \{#([a-z0-9-]+)\}<\/h\1>/g,
    '<h$1 id="$3">$2</h$1>');
}

function renderChapterContents(markdown: string): string {
  const headings = [...markdown.matchAll(/^## (.+?)(?: \{#([a-z0-9-]+)\})?$/gm)];
  if (!headings.length) return '';
  return `<nav class="chapter-contents" aria-label="On this page"><span>On this page</span>${headings.map(([, title, id]) => `<a href="#${id || docsHeadingSlug(title)}">${escapeHtml(title)}</a>`).join('')}</nav>`;
}

function uniqueSources(checkpoints: StaticChapterCheckpoint[]): StaticChapterCheckpoint[] {
  const bySource = new Map<string, StaticChapterCheckpoint>();
  for (const checkpoint of checkpoints) {
    const existing = bySource.get(checkpoint.source);
    if (existing) {
      bySource.set(checkpoint.source, { ...existing, id: `${existing.id} ${checkpoint.id}` });
    } else {
      bySource.set(checkpoint.source, checkpoint);
    }
  }
  return [...bySource.values()];
}

function renderSource(checkpoint: StaticChapterCheckpoint): string {
  const asset = checkpoint.assetPath
    ? `<figure><img src="${escapeHtml(checkpoint.assetPath)}" alt="Rendered ${escapeHtml(checkpoint.id)} checkpoint" /></figure>`
    : '';
  return `<article class="docs-chapter-source" data-checkpoint-id="${escapeHtml(checkpoint.id)}" data-checkpoint-source="${escapeHtml(checkpoint.source)}">
    <h3><code>${escapeHtml(checkpoint.source)}</code></h3>${asset}
    <pre><code class="language-ecky">${escapeHtml(checkpoint.code)}</code></pre>
  </article>`;
}

function renderPager(previous: StaticChapter | null, next: StaticChapter | null, options: ChaptersSiteOptions): string {
  const previousLink = previous ? `<a class="docs-pager__link docs-pager__link--previous" href="${chapterRoute(previous, options)}"><span>Previous</span><strong>← ${escapeHtml(previous.title)}</strong></a>` : '<span></span>';
  const nextLink = next ? `<a class="docs-pager__link docs-pager__link--next" href="${chapterRoute(next, options)}"><span>Next</span><strong>${escapeHtml(next.title)} →</strong></a>` : '<span></span>';
  return `<nav class="docs-pager" aria-label="Chapter navigation">${previousLink}${nextLink}</nav>`;
}

function chapterRoute(chapter: StaticChapter, options: ChaptersSiteOptions): string {
  return `${normalizeBasePath(options.basePath)}/chapters/${chapter.sectionSlug}/`;
}

function normalizeBasePath(basePath: string): string {
  return `/${basePath.replace(/^\/+|\/+$/g, '')}`;
}

function renderChapterStylesheet(): string {
  return `
  :root { --bg:#1a1a2e; --bg-100:#16213e; --bg-200:#111524; --bg-300:#2a2a4a; --text:#e0e0e0; --text-dim:#8a8aa8; --primary:#c8924f; --secondary:#e1b766; --border:#2a2a4a; --border-bright:#3a3a5a; --header-h:58px; --font-mono:'Fira Code','SF Mono',ui-monospace,monospace; --font-display:'Space Grotesk','Fira Code',sans-serif; }
  * { box-sizing:border-box; border-radius:0; margin:0; } html { scroll-behavior:smooth; scroll-padding-top:84px; } body { min-height:100vh; overflow-x:hidden; background:var(--bg); color:var(--text); font:18px/1.75 system-ui,-apple-system,sans-serif; background-image:linear-gradient(rgba(74,140,92,.025) 1px,transparent 1px),linear-gradient(90deg,rgba(74,140,92,.025) 1px,transparent 1px); background-size:24px 24px; } a { color:var(--primary); text-decoration:none; } a:hover { text-decoration:underline; } code { padding:.08em .32em; background:rgba(200,166,32,.1); color:var(--secondary); font-family:var(--font-mono); }
  .docs-header { position:sticky; top:0; z-index:50; height:var(--header-h); overflow:hidden; border-bottom:1px solid var(--border); background:rgba(26,26,46,.96); } .docs-header__inner { width:min(1180px,100%); height:100%; margin:0 auto; padding:0 1.25rem; display:flex; align-items:center; gap:.6rem; overflow:hidden; font-size:.8rem; } .docs-header__home { color:var(--text-dim); white-space:nowrap; } .docs-header__sep { color:var(--bg-300); } .docs-header__title { overflow:hidden; color:var(--text); font-family:var(--font-display); font-weight:600; text-overflow:ellipsis; white-space:nowrap; } .docs-header__actions { margin-left:auto; display:flex; gap:.5rem; } .docs-action { padding:.3rem .6rem; border:1px solid var(--border-bright); color:var(--text); font-size:.68rem; letter-spacing:.04em; text-transform:uppercase; white-space:nowrap; } .docs-action--primary { border-color:var(--primary); color:var(--primary); }
  .docs-shell,.docs-layout { overflow:hidden; } .docs-layout { width:min(900px,100%); min-height:calc(100vh - var(--header-h)); margin:0 auto; padding:0 1.25rem 4rem; } .docs-main { min-width:0; max-width:760px; overflow:hidden; padding:3rem 0 1rem; } .docs-main--chapters { max-width:720px; margin-inline:auto; } .docs-main__section { overflow:hidden; } .docs-main__eyebrow { margin-bottom:.5rem; color:var(--secondary); font-size:.66rem; letter-spacing:.12em; } .docs-main__heading { margin-bottom:1.7rem; padding-bottom:.8rem; border-bottom:1px solid var(--border-bright); font:700 clamp(1.75rem,5vw,2.45rem)/1.1 var(--font-display); letter-spacing:-.03em; } .docs-main__body p { margin-bottom:1rem; } .docs-main__body ul { margin:0 0 1.2rem; padding-left:1.4rem; } .docs-main__body li { margin-bottom:.4rem; } .docs-main__body h3,.docs-main__body h4 { scroll-margin-top:82px; } .docs-main__body h3 { margin:2rem 0 .8rem; color:var(--primary); font:600 1.08rem var(--font-display); } .docs-main__body h4 { margin:1.5rem 0 .65rem; color:var(--secondary); font:600 .95rem var(--font-display); } .docs-main__body pre,.docs-chapter-source pre { max-width:100%; overflow-x:auto; margin:1rem 0 1.4rem; padding:1rem 1.1rem; border:1px solid var(--border); background:var(--bg-200); color:#c8d8c8; font-size:.83rem; line-height:1.55; } .docs-main__body pre code,.docs-chapter-source pre code { padding:0; background:none; color:inherit; } .docs-main__body table { width:100%; margin:1rem 0 1.4rem; border-collapse:collapse; font-size:.82rem; } .docs-main__body th,.docs-main__body td { padding:.52rem .68rem; border:1px solid var(--border); text-align:left; vertical-align:top; } .docs-main__body th { background:rgba(200,166,32,.08); color:var(--secondary); } .docs-main__body figure,.docs-chapter-source figure { margin:1.2rem 0 1.6rem; overflow:hidden; } .docs-main__body figure img,.docs-chapter-source figure img { display:block; width:100%; height:auto; border:1px solid var(--border); }
  .docs-chapter-sources { margin-top:3rem; overflow:hidden; border-top:1px solid var(--border); padding-top:1.4rem; } .docs-chapter-sources > h2 { margin-bottom:.7rem; color:var(--primary); font:600 1.2rem var(--font-display); } .docs-chapter-sources > p { margin-bottom:1.5rem; color:var(--text-dim); } .docs-chapter-source { margin-bottom:2.25rem; overflow:hidden; } .docs-chapter-source h3 { color:var(--secondary); font:500 .78rem var(--font-mono); overflow-wrap:anywhere; } .docs-chapter-source h3 code { overflow-wrap:anywhere; }
  .docs-pager { margin-top:3rem; display:grid; grid-template-columns:minmax(0,1fr) minmax(0,1fr); gap:.8rem; overflow:hidden; border-top:1px solid var(--border); padding-top:1rem; } .docs-pager--chapters { grid-template-columns:repeat(2,minmax(0,1fr)); } .docs-pager__link { min-width:0; padding:.8rem; overflow:hidden; border:1px solid var(--border); background:var(--bg-100); } .docs-pager__link:hover { border-color:var(--primary); text-decoration:none; } .docs-pager__link span { display:block; color:var(--text-dim); font-size:.65rem; text-transform:uppercase; } .docs-pager__link strong { display:block; overflow:hidden; color:var(--text); font-size:.78rem; text-overflow:ellipsis; white-space:normal; } .docs-pager__link--next { text-align:right; } .docs-footer { max-width:900px; margin:0 auto; padding:1.4rem 1.25rem; display:flex; justify-content:space-between; overflow:hidden; border-top:1px solid var(--border); color:var(--text-dim); font-size:.72rem; }
  .chapter-contents { display:grid; gap:.4rem; margin:0 0 2.5rem; padding:1.2rem 0; border-bottom:1px solid var(--border); font-size:.9rem; overflow:hidden; }
  .chapter-contents span { color:var(--text-dim); font: .7rem var(--font-mono); text-transform:uppercase; letter-spacing:.08em; }
  .docs-main__body h3 { font-size:1.35rem; margin-top:2.8rem; color:var(--text); }
  .docs-main__body p code { font-size:.82em; overflow-wrap:anywhere; }
  .docs-main__body a { text-decoration:underline; text-underline-offset:3px; }
  .docs-pager__link strong { font:500 1rem/1.5 var(--font-display); }
  @media (max-width:860px) { .docs-header__inner { padding:0 .7rem; gap:.45rem; } .docs-header__home,.docs-header__sep { display:none; } .docs-header__title { font-size:.78rem; } .docs-action { padding:.28rem .42rem; font-size:.62rem; } .docs-layout,.docs-main,.docs-main__section { overflow:visible; } .docs-layout { min-height:0; padding:0 1rem 3rem; } .docs-main { max-width:none; padding-top:1.8rem; } .docs-pager,.docs-pager--chapters { grid-template-columns:1fr; } }
  `;
}

function escapeHtml(value: string): string {
  return value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
}
