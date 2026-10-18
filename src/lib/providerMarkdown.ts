import { Marked, type Tokens } from 'marked';
import {
  parseProviderCodeReference,
  providerMathTokenAtStart,
  type ProviderCodeReference,
} from './providerMessagePresentation';
import { renderProviderMath } from './providerMath';
import { docsDownloadFilename } from './docs/eckyIrGuide';

export type ProviderMermaidDiagram = {
  id: string;
  source: string;
};

export type ProviderMarkdownRender = {
  html: string;
  diagrams: ProviderMermaidDiagram[];
  codeReferences: ProviderCodeReference[];
};

type MarkedMathToken = Tokens.Generic & {
  latex: string;
  displayMode: boolean;
};

function escapeHtml(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function safeExternalUrl(value: string): string | null {
  const trimmed = value.trim();
  if (trimmed.startsWith('#')) return trimmed;
  try {
    const url = new URL(trimmed);
    return ['http:', 'https:', 'mailto:'].includes(url.protocol) ? url.href : null;
  } catch {
    return null;
  }
}

function blockMathStart(src: string): number | undefined {
  const matches = [src.indexOf('$$'), src.indexOf('\\[')]
    .filter((index) => index >= 0);
  return matches.length > 0 ? Math.min(...matches) : undefined;
}

function inlineMathStart(src: string): number | undefined {
  const parenthesized = src.indexOf('\\(');
  let dollar = -1;
  for (let index = src.indexOf('$'); index >= 0; index = src.indexOf('$', index + 1)) {
    if (src[index + 1] !== '$') {
      dollar = index;
      break;
    }
  }
  const matches = [parenthesized, dollar].filter((index) => index >= 0);
  return matches.length > 0 ? Math.min(...matches) : undefined;
}

export function renderProviderMarkdown(content: string): ProviderMarkdownRender {
  const diagrams: ProviderMermaidDiagram[] = [];
  const codeReferences: ProviderCodeReference[] = [];
  const marked = new Marked({
    gfm: true,
    breaks: true,
    renderer: {
      html({ text }) {
        return escapeHtml(text);
      },
      code({ text, lang }) {
        const language = `${lang ?? ''}`.trim().split(/\s+/)[0].toLowerCase();
        if (language === 'mermaid') {
          const id = `provider-mermaid-${diagrams.length}`;
          diagrams.push({ id, source: text });
          return `<div class="provider-mermaid" data-provider-mermaid-id="${id}" role="img" aria-label="Mermaid diagram"><div class="provider-mermaid-pending" role="status">RENDERING DIAGRAM…</div></div>`;
        }
        const className = language ? ` class="language-${escapeHtml(language)}"` : '';
        return `<pre><code${className}>${escapeHtml(text)}</code></pre>`;
      },
      link({ href, title, text, tokens }) {
        const reference = parseProviderCodeReference(text, href);
        const labelHtml = this.parser.parseInline(tokens);
        if (reference) {
          const index = codeReferences.length;
          codeReferences.push(reference);
          return `<button type="button" class="provider-code-reference" data-provider-code-reference="${index}" aria-label="Open ${escapeHtml(reference.label)} at line ${reference.line}"><span>${labelHtml}</span><small>LINE ${reference.line}</small></button>`;
        }
        const download = docsDownloadFilename(href);
        if (download) return `<a href="${escapeHtml(href)}" download="${escapeHtml(download)}">${labelHtml}</a>`;
        const safeHref = safeExternalUrl(href);
        if (!safeHref) return labelHtml;
        const titleAttribute = title ? ` title="${escapeHtml(title)}"` : '';
        return `<a href="${escapeHtml(safeHref)}" target="_blank" rel="noopener noreferrer"${titleAttribute}>${labelHtml}</a>`;
      },
      image({ href, title, text }) {
        const safeHref = safeExternalUrl(href);
        if (!safeHref || !['http:', 'https:'].includes(new URL(safeHref).protocol)) {
          return escapeHtml(text);
        }
        const titleAttribute = title ? ` title="${escapeHtml(title)}"` : '';
        return `<img src="${escapeHtml(safeHref)}" alt="${escapeHtml(text)}" loading="lazy"${titleAttribute}>`;
      },
    },
    extensions: [
      {
        name: 'providerBlockMath',
        level: 'block',
        start: blockMathStart,
        tokenizer(src) {
          const token = providerMathTokenAtStart(src);
          if (!token?.displayMode) return undefined;
          return { type: 'providerBlockMath', ...token };
        },
        renderer(token) {
          const math = token as MarkedMathToken;
          return `<div class="provider-math provider-math--display">${renderProviderMath(math.latex, true)}</div>`;
        },
      },
      {
        name: 'providerInlineMath',
        level: 'inline',
        start: inlineMathStart,
        tokenizer(src) {
          const token = providerMathTokenAtStart(src);
          if (!token || token.displayMode) return undefined;
          return { type: 'providerInlineMath', ...token };
        },
        renderer(token) {
          const math = token as MarkedMathToken;
          return `<span class="provider-math">${renderProviderMath(math.latex, false)}</span>`;
        },
      },
    ],
  });

  return {
    html: marked.parse(content, { async: false }),
    diagrams,
    codeReferences,
  };
}
