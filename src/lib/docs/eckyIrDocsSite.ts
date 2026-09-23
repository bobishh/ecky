import {
  docsHeadingSlug,
  resolveSection,
  type DocsDocument,
  type DocsSection,
} from './eckyIrGuide';

export type DocsSiteOptions = {
  basePath: string;
  rawMarkdownPath: string;
  epubPath: string;
};

export function buildDocsSitePages(
  doc: DocsDocument,
  options: DocsSiteOptions,
): Map<string, string> {
  const pages = new Map<string, string>();
  doc.sections.forEach((section, index) => {
    const outputPath = index === 0 ? 'index.html' : `${section.slug}/index.html`;
    pages.set(outputPath, buildDocsSiteHtml(doc, options, section.slug));
  });
  return pages;
}

export function buildDocsSiteHtml(
  doc: DocsDocument,
  options: DocsSiteOptions,
  activeSlug?: string,
): string {
  const active = resolveSection(doc.sections, activeSlug);
  if (!active) throw new Error('Docs reference has no sections');

  const activeIndex = doc.sections.findIndex((section) => section.slug === active.slug);
  const previous = activeIndex > 0 ? doc.sections[activeIndex - 1] : null;
  const next = activeIndex < doc.sections.length - 1 ? doc.sections[activeIndex + 1] : null;
  const referenceRoutes = buildReferenceRoutes(doc.sections, options);
  const bodyHtml = rewriteReferenceLinks(active.bodyHtml, referenceRoutes);

  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>${escapeHtml(active.title)} · ${escapeHtml(doc.title)}</title>
    <meta name="description" content="Ecky language forms, signatures, selectors, and verification grammar." />
    <meta name="robots" content="index, follow" />
    <link rel="icon" href="/favicon.svg" type="image/svg+xml" />
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link href="https://fonts.googleapis.com/css2?family=Fira+Code:wght@400;500;600&family=Space+Grotesk:wght@500;600;700&display=swap" rel="stylesheet" />
    <style>${renderStylesheet()}</style>
    <script src="${escapeHtml(normalizeBasePath(options.basePath))}/docs.js" defer></script>
  </head>
  <body>
    <header class="docs-header">
      <div class="docs-header__inner">
        <button class="docs-menu" type="button" aria-controls="docs-toc" aria-expanded="false">☰ Contents</button>
        <a class="docs-header__home" href="/">← Ecky CAD</a>
        <span class="docs-header__sep">/</span>
        <a class="docs-header__title" href="${sectionRoute(doc.sections[0], doc.sections, options)}">${escapeHtml(doc.title)}</a>
        <nav class="docs-header__actions" aria-label="Documentation">
          <a class="docs-action" href="${normalizeBasePath(options.basePath)}/chapters/">Chapters</a>
          <a class="docs-action" href="${normalizeBasePath(options.basePath)}/">Reference</a>
          <a class="docs-action docs-action--raw" href="${escapeHtml(options.rawMarkdownPath)}" type="text/markdown">Raw .md</a>
          <a class="docs-action docs-action--primary" href="${escapeHtml(options.epubPath)}">EPUB ↓</a>
        </nav>
      </div>
    </header>

    <div class="docs-shell">
      <button class="docs-backdrop" type="button" aria-label="Close contents" tabindex="-1"></button>
      <div class="docs-layout">
        <nav class="docs-toc" id="docs-toc" aria-label="Reference contents">
          <div class="docs-toc__head">
            <span>${escapeHtml(doc.title)}</span>
            <button class="docs-toc__close" type="button" aria-label="Close contents">×</button>
          </div>
          <div class="docs-toc__list">${doc.sections
            .map((section) => renderTocItem(section, active, doc.sections, options))
            .join('')}</div>
        </nav>

        <main class="docs-main">
          ${activeIndex === 0 ? `<div class="docs-summary">${doc.summaryHtml}</div>` : ''}
          <section class="docs-main__section" id="${active.slug}">
            <p class="docs-main__eyebrow">REFERENCE / ${String(activeIndex + 1).padStart(2, '0')}</p>
            <h1 class="docs-main__heading">${escapeHtml(active.title)}${renderStatus(active)}</h1>
            <div class="docs-main__body">${bodyHtml}</div>
          </section>
          ${renderPager(previous, next, doc.sections, options)}
        </main>
      </div>
    </div>

    <footer class="docs-footer">
      <span>${escapeHtml(doc.title)}</span>
      <a href="${escapeHtml(options.rawMarkdownPath)}">Raw Markdown</a>
    </footer>
  </body>
</html>`;
}

export function buildDocsClientScript(): string {
  return `(function () {
  var root = document.documentElement;
  var shell = document.querySelector('.docs-shell');
  var menu = document.querySelector('.docs-menu');
  var close = document.querySelector('.docs-toc__close');
  var backdrop = document.querySelector('.docs-backdrop');
  if (!shell || !menu) return;

  function setOpen(open) {
    shell.classList.toggle('docs-shell--nav-open', open);
    root.classList.toggle('docs-nav-open', open);
    menu.setAttribute('aria-expanded', open ? 'true' : 'false');
  }

  menu.addEventListener('click', function () {
    setOpen(menu.getAttribute('aria-expanded') !== 'true');
  });
  if (close) close.addEventListener('click', function () { setOpen(false); });
  if (backdrop) backdrop.addEventListener('click', function () { setOpen(false); });
  document.addEventListener('keydown', function (event) {
    if (event.key === 'Escape') setOpen(false);
  });

  if (window.location.pathname === '/docs/' && window.location.hash) {
    var slug = window.location.hash.slice(1);
    var legacyTarget = document.querySelector('[data-section-slug="' + slug + '"]');
    if (legacyTarget && legacyTarget.getAttribute('href')) {
      window.location.replace(legacyTarget.getAttribute('href'));
    }
  }
})();`;
}

function renderTocItem(
  section: DocsSection,
  active: DocsSection,
  sections: DocsSection[],
  options: DocsSiteOptions,
): string {
  const current = section.slug === active.slug ? ' aria-current="page"' : '';
  return `<a class="docs-toc__link" data-section-slug="${section.slug}" href="${sectionRoute(section, sections, options)}"${current}>
    <span class="docs-toc__label">${escapeHtml(section.title)}</span>
    ${section.status === 'pending' ? '<span class="docs-status">pending</span>' : ''}
  </a>`;
}

function renderStatus(section: DocsSection): string {
  return section.status === 'pending' ? '<span class="docs-status">Pending</span>' : '';
}

function renderPager(
  previous: DocsSection | null,
  next: DocsSection | null,
  sections: DocsSection[],
  options: DocsSiteOptions,
): string {
  const previousLink = previous
    ? `<a class="docs-pager__link docs-pager__link--previous" href="${sectionRoute(previous, sections, options)}"><span>Previous</span><strong>← ${escapeHtml(previous.title)}</strong></a>`
    : '<span></span>';
  const nextLink = next
    ? `<a class="docs-pager__link docs-pager__link--next" href="${sectionRoute(next, sections, options)}"><span>Next</span><strong>${escapeHtml(next.title)} →</strong></a>`
    : '<span></span>';
  return `<nav class="docs-pager" aria-label="Section navigation">${previousLink}${nextLink}</nav>`;
}

function buildReferenceRoutes(
  sections: DocsSection[],
  options: DocsSiteOptions,
): Map<string, string> {
  const routes = new Map<string, string>();
  for (const section of sections) {
    const route = sectionRoute(section, sections, options);
    const headings = section.bodyMarkdown.matchAll(/^#{3,4}\s+(.+)$/gm);
    for (const heading of headings) {
      const slug = docsHeadingSlug(heading[1] ?? '');
      if (slug) routes.set(slug, `${route}#${slug}`);
    }
  }
  return routes;
}

function rewriteReferenceLinks(html: string, routes: Map<string, string>): string {
  return html.replace(/href="#([a-z0-9_-]+)"/g, (match, slug: string) => {
    const route = routes.get(slug);
    return route ? `href="${route}"` : match;
  });
}

function sectionRoute(
  section: DocsSection,
  sections: DocsSection[],
  options: DocsSiteOptions,
): string {
  const base = normalizeBasePath(options.basePath);
  return section.slug === sections[0]?.slug ? `${base}/` : `${base}/${section.slug}/`;
}

function normalizeBasePath(basePath: string): string {
  return `/${basePath.replace(/^\/+|\/+$/g, '')}`;
}

function renderStylesheet(): string {
  return `
  :root {
    --bg: #1a1a2e;
    --bg-100: #16213e;
    --bg-200: #111524;
    --bg-300: #2a2a4a;
    --text: #e0e0e0;
    --text-dim: #8a8aa8;
    --primary: #c8924f;
    --secondary: #e1b766;
    --border: #2a2a4a;
    --border-bright: #3a3a5a;
    --header-h: 58px;
    --font-mono: 'Fira Code', 'SF Mono', 'Cascadia Code', ui-monospace, monospace;
    --font-display: 'Space Grotesk', 'Fira Code', sans-serif;
  }

  * { box-sizing: border-box; border-radius: 0; margin: 0; }
  html { scroll-behavior: smooth; scroll-padding-top: 84px; }
  body {
    min-height: 100vh;
    overflow-x: hidden;
    background: var(--bg);
    color: var(--text);
    font: 18px/1.75 system-ui, -apple-system, sans-serif;
    background-image:
      linear-gradient(rgba(74, 140, 92, 0.025) 1px, transparent 1px),
      linear-gradient(90deg, rgba(74, 140, 92, 0.025) 1px, transparent 1px);
    background-size: 24px 24px;
    -webkit-font-smoothing: antialiased;
  }
  a { color: var(--primary); text-decoration: none; }
  a:hover { text-decoration: underline; }
  code {
    padding: 0.08em 0.32em;
    background: rgba(200, 166, 32, 0.10);
    color: var(--secondary);
    font-family: var(--font-mono);
  }

  .docs-header {
    position: sticky;
    top: 0;
    z-index: 50;
    height: var(--header-h);
    overflow: hidden;
    border-bottom: 1px solid var(--border);
    background: rgba(26, 26, 46, 0.96);
    backdrop-filter: blur(12px);
  }
  .docs-header__inner {
    width: min(1280px, 100%);
    height: 100%;
    margin: 0 auto;
    padding: 0 1.25rem;
    display: flex;
    align-items: center;
    gap: 0.6rem;
    overflow: hidden;
    font-size: 0.8rem;
  }
  .docs-header__home { color: var(--text-dim); white-space: nowrap; }
  .docs-header__sep { color: var(--bg-300); }
  .docs-header__title {
    overflow: hidden;
    color: var(--text);
    font-family: var(--font-display);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .docs-header__actions { margin-left: auto; display: flex; gap: 0.5rem; }
  .docs-action {
    padding: 0.3rem 0.6rem;
    border: 1px solid var(--border-bright);
    color: var(--text);
    font-size: 0.68rem;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    white-space: nowrap;
  }
  .docs-action--primary { border-color: var(--primary); color: var(--primary); }
  .docs-menu {
    display: none;
    border: 1px solid var(--border-bright);
    background: var(--bg-100);
    color: var(--text);
    padding: 0.35rem 0.55rem;
    font: 600 0.72rem var(--font-mono);
    text-transform: uppercase;
  }

  .docs-shell, .docs-layout { overflow: hidden; }
  .docs-layout {
    width: min(1180px, 100%);
    min-height: calc(100vh - var(--header-h));
    margin: 0 auto;
    padding: 0 1.25rem 4rem;
    display: grid;
    grid-template-columns: 260px minmax(0, 760px);
    gap: 3rem;
  }
  .docs-toc {
    position: sticky;
    top: calc(var(--header-h) + 24px);
    align-self: start;
    max-height: calc(100vh - var(--header-h) - 48px);
    overflow-y: auto;
    padding: 1.4rem 0.5rem 1.4rem 0;
  }
  .docs-toc__head {
    padding: 0 0.6rem 0.7rem;
    border-bottom: 1px solid var(--border);
    color: var(--secondary);
    font: 600 0.7rem var(--font-display);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .docs-toc__close { display: none; }
  .docs-toc__list { padding-top: 0.6rem; }
  .docs-toc__link {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.48rem 0.65rem;
    border-left: 2px solid transparent;
    color: var(--text-dim);
    font-size: 0.8rem;
    line-height: 1.3;
  }
  .docs-toc__link:hover { background: rgba(255,255,255,0.03); color: var(--text); text-decoration: none; }
  .docs-toc__link[aria-current="page"] {
    border-left-color: var(--primary);
    background: rgba(74, 140, 92, 0.09);
    color: var(--primary);
  }
  .docs-status {
    margin-left: 0.65rem;
    padding: 0.08rem 0.3rem;
    border: 1px solid #8a7215;
    color: var(--secondary);
    font: 500 0.58rem var(--font-mono);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .docs-backdrop { display: none; }

  .docs-main {
    min-width: 0;
    max-width: 760px;
    overflow: hidden;
    padding: 3rem 0 1rem;
  }
  .docs-summary {
    margin-bottom: 2rem;
    padding: 1rem 1.1rem;
    overflow: hidden;
    border-left: 2px solid var(--secondary);
    background: rgba(200, 166, 32, 0.06);
    color: var(--text-dim);
    font-size: 0.88rem;
  }
  .docs-summary p + p { margin-top: 0.55rem; }
  .docs-main__section { overflow: hidden; }
  .docs-main__eyebrow {
    margin-bottom: 0.5rem;
    color: var(--secondary);
    font-size: 0.66rem;
    letter-spacing: 0.12em;
  }
  .docs-main__heading {
    margin-bottom: 1.7rem;
    padding-bottom: 0.8rem;
    border-bottom: 1px solid var(--border-bright);
    font: 700 clamp(1.75rem, 5vw, 2.45rem)/1.1 var(--font-display);
    letter-spacing: -0.03em;
  }
  .docs-main__body p { margin-bottom: 1rem; }
  .docs-main__body ul { margin: 0 0 1.2rem; padding-left: 1.4rem; }
  .docs-main__body li { margin-bottom: 0.4rem; }
  .docs-main__body h3, .docs-main__body h4 { scroll-margin-top: 82px; }
  .docs-main__body h3 {
    margin: 2rem 0 0.8rem;
    color: var(--primary);
    font: 600 1.08rem var(--font-display);
  }
  .docs-main__body h4 {
    margin: 1.5rem 0 0.65rem;
    color: var(--secondary);
    font: 600 0.95rem var(--font-display);
  }
  .docs-main__body pre {
    max-width: 100%;
    overflow-x: auto;
    margin: 1rem 0 1.4rem;
    padding: 1rem 1.1rem;
    border: 1px solid var(--border);
    background: var(--bg-200);
    color: #c8d8c8;
    font-size: 0.83rem;
    line-height: 1.55;
  }
  .docs-main__body pre code { padding: 0; background: none; color: inherit; }
  .docs-main__body table {
    width: 100%;
    margin: 1rem 0 1.4rem;
    border-collapse: collapse;
    font-size: 0.82rem;
  }
  .docs-main__body th, .docs-main__body td {
    padding: 0.52rem 0.68rem;
    border: 1px solid var(--border);
    text-align: left;
    vertical-align: top;
  }
  .docs-main__body th { background: rgba(200, 166, 32, 0.08); color: var(--secondary); }
  .docs-main__body tbody tr:hover { background: rgba(74, 140, 92, 0.05); }
  .docs-main__body figure { margin: 1.2rem 0 1.6rem; overflow: hidden; }
  .docs-main__body figure img { display: block; width: 100%; height: auto; border: 1px solid var(--border); }
  .docs-main__body figcaption { margin-top: 0.5rem; color: var(--text-dim); font-size: 0.8rem; }

  .docs-pager {
    margin-top: 3rem;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 0.8rem;
    overflow: hidden;
    border-top: 1px solid var(--border);
    padding-top: 1rem;
  }
  .docs-pager__link {
    min-width: 0;
    padding: 0.8rem;
    overflow: hidden;
    border: 1px solid var(--border);
    background: var(--bg-100);
  }
  .docs-pager__link:hover { border-color: var(--primary); text-decoration: none; }
  .docs-pager__link span { display: block; color: var(--text-dim); font-size: 0.65rem; text-transform: uppercase; }
  .docs-pager__link strong { display: block; overflow: hidden; color: var(--text); font-size: 0.78rem; text-overflow: ellipsis; white-space: nowrap; }
  .docs-pager__link--next { text-align: right; }

  .docs-footer {
    max-width: 1180px;
    margin: 0 auto;
    padding: 1.4rem 1.25rem;
    display: flex;
    justify-content: space-between;
    overflow: hidden;
    border-top: 1px solid var(--border);
    color: var(--text-dim);
    font-size: 0.72rem;
  }

  .docs-main__body p code, .docs-main__body li code { font-size: .82em; overflow-wrap: anywhere; }
  .docs-main__body h3 { font-size: 1.4rem; margin-top: 2.6rem; color: var(--text); }
  .docs-main__body ul, .docs-main__body ol { padding-left: 1.3rem; margin-bottom: 1.3rem; }
  .docs-main__body table { display: block; overflow-x: auto; font-size: .88rem; }
  .docs-main__body th, .docs-main__body td { min-width: 110px; }
  .docs-main__body pre { font-size: .82rem; }
  @media (max-width: 860px) {
    html.docs-nav-open,
    html.docs-nav-open body { overflow: hidden; overscroll-behavior: none; }
    .docs-header__inner { padding: 0 0.7rem; gap: 0.45rem; }
    .docs-menu { display: inline-flex; }
    .docs-header__home, .docs-header__sep { display: none; }
    .docs-header__title { font-size: 0.78rem; }
    .docs-action { padding: 0.28rem 0.42rem; font-size: 0.62rem; }
    .docs-action--primary, .docs-action--raw { display: none; }
    .docs-shell,
    .docs-layout,
    .docs-main,
    .docs-main__section { overflow: visible; }
    .docs-layout { display: block; min-height: 0; padding: 0 1rem 3rem; }
    .docs-main { max-width: none; padding-top: 1.8rem; }
    .docs-summary { margin-bottom: 1.4rem; }

    .docs-toc {
      position: fixed;
      z-index: 80;
      top: var(--header-h);
      bottom: 0;
      left: 0;
      width: min(330px, 88vw);
      height: calc(100vh - var(--header-h));
      height: calc(100dvh - var(--header-h));
      max-height: calc(100vh - var(--header-h));
      max-height: calc(100dvh - var(--header-h));
      padding: 0;
      overflow-y: auto;
      overscroll-behavior-y: contain;
      touch-action: pan-y;
      -webkit-overflow-scrolling: touch;
      border-right: 1px solid var(--border-bright);
      background: var(--bg-100);
      transform: translateX(-102%);
      visibility: hidden;
      transition: transform 0.16s ease, visibility 0.16s;
    }
    .docs-shell--nav-open .docs-toc { transform: translateX(0); visibility: visible; }
    .docs-toc__head {
      position: sticky;
      top: 0;
      z-index: 2;
      padding: 0.8rem 0.9rem;
      display: flex;
      align-items: center;
      justify-content: space-between;
      background: var(--bg-100);
    }
    .docs-toc__close {
      display: block;
      border: 0;
      background: transparent;
      color: var(--text);
      font: 1.4rem/1 var(--font-mono);
    }
    .docs-toc__list { padding: 0.5rem 0.6rem 1.5rem; }
    .docs-toc__link { min-height: 44px; align-items: center; padding: 0.7rem 0.75rem; }
    .docs-backdrop {
      position: fixed;
      z-index: 70;
      inset: var(--header-h) 0 0;
      width: 100%;
      border: 0;
      background: rgba(8, 9, 18, 0.68);
    }
    .docs-shell--nav-open .docs-backdrop { display: block; }
    .docs-pager { grid-template-columns: 1fr; }
  }
  `;
}

function escapeHtml(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');
}
