/**
 * Build the Ecky IR Field Guide as a standalone, server-rendered HTML page.
 *
 * Unlike the Svelte landing (which needs Three.js for the mascot), the docs are
 * static text + code + images. A build-time HTML render is strictly better:
 * fast for users, crawlable for SEO, and zero JS payload for the core reading
 * experience. The raw markdown is served separately at /docs/ecky-ir.md for
 * agents/LLMs.
 *
 * The parser (parseDocsDocument) and this template share the same source —
 * no drift from the in-app reader.
 */
import type { DocsDocument, DocsSection } from './eckyIrGuide';

export type DocsSiteOptions = {
  /** Path to the raw markdown source, served for agents/LLMs. */
  rawMarkdownPath: string;
  /** Path to the EPUB download. */
  epubPath: string;
};

/**
 * Render the full docs site as one self-contained HTML document.
 * Single-page: a sticky sidebar TOC + all sections on one scroll surface,
 * with anchor links and scroll-spy active highlighting.
 */
export function buildDocsSiteHtml(doc: DocsDocument, options: DocsSiteOptions): string {
  const tocItems = doc.sections.map(renderTocItem).join('');
  const sectionsHtml = doc.sections.map(renderSection).join('');

  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>${escapeHtml(doc.title)}</title>
    <meta name="description" content="The Ecky IR Field Guide: prompt-driven CAD language reference, from first solid to complete multi-part models." />
    <meta name="robots" content="index, follow" />
    <meta property="og:type" content="article" />
    <meta property="og:title" content="${escapeHtml(doc.title)}" />
    <meta property="og:description" content="Prompt-driven CAD language reference, from first solid to complete multi-part models." />
    <link rel="icon" href="/favicon.svg" type="image/svg+xml" />
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link href="https://fonts.googleapis.com/css2?family=Fira+Code:wght@400;500;600&family=Space+Grotesk:wght@500;600;700&display=swap" rel="stylesheet" />
    <style>${renderStylesheet()}</style>
  </head>
  <body>
    <header class="docs-header">
      <div class="docs-header__inner">
        <a class="docs-header__home" href="/">← Ecky CAD</a>
        <span class="docs-header__sep">/</span>
        <span class="docs-header__title">${escapeHtml(doc.title)}</span>
        <div class="docs-header__actions">
          <a class="docs-action" href="${escapeHtml(options.rawMarkdownPath)}" type="text/markdown">Raw .md</a>
          <a class="docs-action docs-action--primary" href="${escapeHtml(options.epubPath)}">EPUB ↓</a>
        </div>
      </div>
    </header>

    <div class="docs-hero">
      <p class="docs-hero__kicker">ECKY LANGUAGE / FIELD GUIDE</p>
      <h1>${escapeHtml(doc.title)}</h1>
      <div class="docs-hero__summary">${doc.summaryHtml}</div>
    </div>

    <div class="docs-layout">
      <nav class="docs-toc" aria-label="Field guide contents">
        <div class="docs-toc__title">Contents</div>
        <div class="docs-toc__list">${tocItems}</div>
      </nav>
      <main class="docs-main">${sectionsHtml}</main>
    </div>

    <footer class="docs-footer">
      <span>${escapeHtml(doc.title)}</span>
      <span class="docs-footer__dim">Ecky CAD — prompt-driven CAD</span>
      <a href="/">ecky-cad.com</a>
    </footer>

    <script>
      // Scroll-spy: highlight the active TOC entry as you scroll.
      (function () {
        var links = document.querySelectorAll('.docs-toc__link');
        var sections = Array.from(document.querySelectorAll('.docs-main__section'));
        if (!('IntersectionObserver' in window) || sections.length === 0) return;
        var observer = new IntersectionObserver(function (entries) {
          entries.forEach(function (entry) {
            if (!entry.isIntersecting) return;
            var id = entry.target.getAttribute('id');
            links.forEach(function (link) {
              link.classList.toggle('docs-toc__link--active', link.getAttribute('href') === '#' + id);
            });
          });
        }, { rootMargin: '-96px 0px -70% 0px', threshold: 0 });
        sections.forEach(function (section) { observer.observe(section); });
      })();
    </script>
  </body>
</html>`;
}

function renderTocItem(section: DocsSection): string {
  const statusBadge = section.status === 'pending'
    ? '<span class="docs-toc__status docs-toc__status--pending">pending</span>'
    : '';
  return `<a class="docs-toc__link" href="#${section.slug}">
    <span class="docs-toc__label">${escapeHtml(section.title)}</span>
    ${statusBadge}
  </a>`;
}

function renderSection(section: DocsSection): string {
  const statusBadge = section.status === 'pending'
    ? '<span class="docs-section__status docs-section__status--pending">Pending</span>'
    : '';
  return `<section class="docs-main__section" id="${section.slug}">
    <h2 class="docs-main__heading">${escapeHtml(section.title)}${statusBadge}</h2>
    <div class="docs-main__body">${section.bodyHtml}</div>
  </section>`;
}

/**
 * Midnight Tactical theme — same tokens as the landing and app.
 * Square borders, mono font, dark palette, green/bronze accents.
 */
function renderStylesheet(): string {
  return `
  :root {
    --bg: #1a1a2e;
    --bg-100: #16213e;
    --bg-200: #111524;
    --bg-300: #2a2a4a;
    --bg-400: #34345e;
    --text: #e0e0e0;
    --text-dim: #8a8aa8;
    --primary: #4a8c5c;
    --primary-dim: #2c5f3c;
    --secondary: #c8a620;
    --secondary-dim: #8a7215;
    --border: #2a2a4a;
    --border-bright: #3a3a5a;
    --font-mono: 'Fira Code', 'SF Mono', 'Cascadia Code', ui-monospace, monospace;
    --font-display: 'Space Grotesk', 'Fira Code', sans-serif;
  }

  * { box-sizing: border-box; border-radius: 0; margin: 0; }

  html { scroll-behavior: smooth; scroll-padding-top: 96px; }

  body {
    background: var(--bg);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 15px;
    line-height: 1.7;
    background-image:
      linear-gradient(rgba(74, 140, 92, 0.03) 1px, transparent 1px),
      linear-gradient(90deg, rgba(74, 140, 92, 0.03) 1px, transparent 1px);
    background-size: 24px 24px;
    -webkit-font-smoothing: antialiased;
  }

  a { color: var(--primary); text-decoration: none; }
  a:hover { text-decoration: underline; }

  code {
    font-family: var(--font-mono);
    color: var(--secondary);
    background: rgba(200, 166, 32, 0.10);
    padding: 0.08em 0.32em;
  }

  /* ── Header bar ── */
  .docs-header {
    position: sticky;
    top: 0;
    z-index: 50;
    border-bottom: 1px solid var(--border);
    background: rgba(26, 26, 46, 0.90);
    backdrop-filter: blur(12px);
  }
  .docs-header__inner {
    max-width: 1240px;
    margin: 0 auto;
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.8rem 1.4rem;
    font-size: 0.82rem;
  }
  .docs-header__home { color: var(--text-dim); }
  .docs-header__home:hover { color: var(--text); text-decoration: none; }
  .docs-header__sep { color: var(--bg-400); }
  .docs-header__title { color: var(--text); font-family: var(--font-display); font-weight: 600; }
  .docs-header__actions { margin-left: auto; display: flex; gap: 0.5rem; }
  .docs-action {
    border: 1px solid var(--border-bright);
    background: var(--bg-100);
    color: var(--text);
    padding: 0.38rem 0.72rem;
    font-size: 0.72rem;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    transition: border-color 0.15s, background 0.15s;
  }
  .docs-action:hover { border-color: var(--text-dim); text-decoration: none; }
  .docs-action--primary {
    border-color: var(--primary);
    background: rgba(74, 140, 92, 0.16);
    color: var(--primary);
  }

  /* ── Hero ── */
  .docs-hero {
    max-width: 760px;
    margin: 0 auto;
    padding: 3.4rem 1.4rem 2.4rem;
    text-align: center;
  }
  .docs-hero__kicker {
    font-size: 0.68rem;
    letter-spacing: 0.14em;
    color: var(--secondary);
    margin-bottom: 0.8rem;
  }
  .docs-hero h1 {
    font-family: var(--font-display);
    font-size: clamp(1.9rem, 5vw, 2.8rem);
    font-weight: 700;
    letter-spacing: -0.03em;
    line-height: 1.05;
    margin-bottom: 1rem;
  }
  .docs-hero__summary { color: var(--text-dim); font-size: 0.96rem; }
  .docs-hero__summary p { margin-bottom: 0.5rem; }

  /* ── Layout ── */
  .docs-layout {
    max-width: 1240px;
    margin: 0 auto;
    display: grid;
    grid-template-columns: 280px minmax(0, 1fr);
    gap: 2rem;
    padding: 0 1.4rem 4rem;
  }

  /* ── TOC sidebar ── */
  .docs-toc {
    position: sticky;
    top: 96px;
    align-self: start;
    max-height: calc(100vh - 112px);
    overflow-y: auto;
    padding-right: 0.5rem;
  }
  .docs-toc__title {
    font-size: 0.68rem;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--secondary);
    margin-bottom: 0.8rem;
    padding-bottom: 0.6rem;
    border-bottom: 1px solid var(--border);
  }
  .docs-toc__link {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.6rem;
    padding: 0.42rem 0.6rem;
    border-left: 2px solid transparent;
    color: var(--text-dim);
    font-size: 0.82rem;
    line-height: 1.35;
    transition: color 0.12s, border-color 0.12s, background 0.12s;
  }
  .docs-toc__link:hover { color: var(--text); text-decoration: none; background: rgba(255,255,255,0.03); }
  .docs-toc__link--active {
    color: var(--primary);
    border-left-color: var(--primary);
    background: rgba(74, 140, 92, 0.08);
  }
  .docs-toc__label { flex: 1; }
  .docs-toc__status, .docs-section__status {
    flex-shrink: 0;
    font-size: 0.6rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    border: 1px solid var(--secondary-dim);
    color: var(--secondary);
    padding: 0.08rem 0.32rem;
  }

  /* ── Main content ── */
  .docs-main { min-width: 0; max-width: 760px; }
  .docs-main__section {
    padding: 1.6rem 0 2.4rem;
    border-bottom: 1px solid var(--border);
  }
  .docs-main__heading {
    display: flex;
    align-items: baseline;
    gap: 0.8rem;
    font-family: var(--font-display);
    font-size: clamp(1.4rem, 3vw, 1.8rem);
    font-weight: 700;
    letter-spacing: -0.02em;
    margin-bottom: 1.2rem;
    padding-bottom: 0.6rem;
    border-bottom: 1px solid var(--border-bright);
  }
  .docs-main__body { color: var(--text); }
  .docs-main__body p { margin-bottom: 1rem; color: var(--text); }
  .docs-main__body ul { margin: 0 0 1.2rem; padding-left: 1.4rem; }
  .docs-main__body li { margin-bottom: 0.4rem; color: var(--text); }
  .docs-main__body h3 {
    margin: 1.8rem 0 0.8rem;
    font-family: var(--font-display);
    font-size: 1.05rem;
    color: var(--primary);
    letter-spacing: 0.02em;
  }
  .docs-main__body h4 {
    margin: 1.4rem 0 0.6rem;
    font-family: var(--font-display);
    font-size: 0.95rem;
    color: var(--secondary);
    letter-spacing: 0.02em;
  }
  .docs-main__body pre {
    overflow-x: auto;
    margin: 1rem 0 1.4rem;
    padding: 1rem 1.2rem;
    border: 1px solid var(--border);
    background: var(--bg-200);
    line-height: 1.55;
    font-size: 0.84rem;
    color: #c8d8c8;
  }
  .docs-main__body pre code { background: none; color: inherit; padding: 0; }
  .docs-main__body figure { margin: 1.2rem 0 1.6rem; }
  .docs-main__body figure img {
    display: block;
    width: 100%;
    height: auto;
    border: 1px solid var(--border);
    background: var(--bg-100);
  }
  .docs-main__body figcaption { margin-top: 0.5rem; font-size: 0.8rem; color: var(--text-dim); }

  /* ── Footer ── */
  .docs-footer {
    border-top: 1px solid var(--border);
    padding: 1.6rem 1.4rem;
  }
  .docs-footer {
    max-width: 1240px;
    margin: 0 auto;
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem;
    font-size: 0.76rem;
    color: var(--text-dim);
  }
  .docs-footer__dim { color: var(--text-dim); }

  /* ── Responsive ── */
  @media (max-width: 860px) {
    .docs-layout { grid-template-columns: 1fr; gap: 0; }
    .docs-toc {
      position: static;
      max-height: none;
      margin-bottom: 1.6rem;
      padding: 1rem;
      border: 1px solid var(--border);
      background: var(--bg-100);
    }
    .docs-toc__list { max-height: 240px; overflow-y: auto; }
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
