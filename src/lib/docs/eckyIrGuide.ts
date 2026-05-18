export type DocsSectionStatus = 'ready' | 'pending';

export type DocsSection = {
  slug: string;
  title: string;
  status: DocsSectionStatus;
  bodyMarkdown: string;
  bodyHtml: string;
  snippet: string | null;
};

export type DocsDocument = {
  title: string;
  summaryHtml: string;
  sections: DocsSection[];
};

const STATUS_SUFFIX = /\s+\[(pending|ready)\]\s*$/i;

export function isDocsRoute(pathname: string): boolean {
  return pathname === '/docs/ecky-ir'
    || pathname === '/learn/ecky-ir'
    || pathname.startsWith('/docs/ecky-ir/')
    || pathname.startsWith('/learn/ecky-ir/');
}

export function docsSourcePath(): string {
  return '/docs/ecky-ir.md';
}

export function parseDocsDocument(markdown: string): DocsDocument {
  const normalized = markdown.replace(/\r\n/g, '\n').trim();
  const sections = splitSections(normalized);
  const titleMatch = normalized.match(/^#\s+(.+)$/m);
  const title = titleMatch?.[1]?.trim() || 'Ecky Language Docs';
  const summaryMarkdown = extractSummary(normalized);

  return {
    title,
    summaryHtml: renderMarkdownFragment(summaryMarkdown),
    sections,
  };
}

export function resolveSection(
  sections: DocsSection[],
  slug: string | null | undefined,
): DocsSection | null {
  if (!sections.length) return null;
  if (!slug) return sections[0];
  return sections.find((section) => section.slug === slug) ?? sections[0];
}

export function renderMarkdownFragment(markdown: string): string {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n');
  const chunks: string[] = [];
  const paragraph: string[] = [];

  function flushParagraph() {
    if (!paragraph.length) return;
    chunks.push(`<p>${renderInline(paragraph.join(' ').trim())}</p>`);
    paragraph.length = 0;
  }

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index] ?? '';
    const trimmed = line.trim();

    if (!trimmed) {
      flushParagraph();
      continue;
    }

    if (trimmed.startsWith('```')) {
      flushParagraph();
      const language = trimmed.slice(3).trim();
      const codeLines: string[] = [];
      index += 1;
      while (index < lines.length && !(lines[index] ?? '').trim().startsWith('```')) {
        codeLines.push(lines[index] ?? '');
        index += 1;
      }
      const className = language ? ` class="language-${escapeHtml(language)}"` : '';
      chunks.push(
        `<pre><code${className}>${escapeHtml(codeLines.join('\n').trimEnd())}</code></pre>`,
      );
      continue;
    }

    if (trimmed.startsWith('### ')) {
      flushParagraph();
      chunks.push(`<h3>${renderInline(trimmed.slice(4).trim())}</h3>`);
      continue;
    }

    if (trimmed.startsWith('#### ')) {
      flushParagraph();
      chunks.push(`<h4>${renderInline(trimmed.slice(5).trim())}</h4>`);
      continue;
    }

    if (trimmed.startsWith('- ')) {
      flushParagraph();
      const items: string[] = [];
      let listIndex = index;
      while (listIndex < lines.length) {
        const candidate = (lines[listIndex] ?? '').trim();
        if (!candidate.startsWith('- ')) break;
        items.push(`<li>${renderInline(candidate.slice(2).trim())}</li>`);
        listIndex += 1;
      }
      chunks.push(`<ul>${items.join('')}</ul>`);
      index = listIndex - 1;
      continue;
    }

    paragraph.push(trimmed);
  }

  flushParagraph();
  return chunks.join('');
}

function splitSections(markdown: string): DocsSection[] {
  const lines = markdown.split('\n');
  const sections: DocsSection[] = [];
  let currentTitle: string | null = null;
  let currentStatus: DocsSectionStatus = 'ready';
  let currentBody: string[] = [];

  function pushCurrent() {
    if (!currentTitle) return;
    const bodyMarkdown = currentBody.join('\n').trim();
    sections.push({
      slug: slugify(currentTitle),
      title: currentTitle,
      status: currentStatus,
      bodyMarkdown,
      bodyHtml: renderMarkdownFragment(bodyMarkdown),
      snippet: extractFirstSnippet(bodyMarkdown),
    });
  }

  for (const line of lines) {
    const headingMatch = line.match(/^##\s+(.+)$/);
    if (!headingMatch) {
      if (currentTitle) currentBody.push(line);
      continue;
    }

    pushCurrent();
    const parsedHeading = parseSectionHeading(headingMatch[1]);
    currentTitle = parsedHeading.title;
    currentStatus = parsedHeading.status;
    currentBody = [];
  }

  pushCurrent();
  return sections;
}

function parseSectionHeading(rawHeading: string): { title: string; status: DocsSectionStatus } {
  const matched = rawHeading.match(STATUS_SUFFIX);
  if (!matched) {
    return { title: rawHeading.trim(), status: 'ready' };
  }

  return {
    title: rawHeading.replace(STATUS_SUFFIX, '').trim(),
    status: matched[1].toLowerCase() as DocsSectionStatus,
  };
}

function extractSummary(markdown: string): string {
  const withoutTitle = markdown.replace(/^#\s+.+$/m, '').trimStart();
  const nextSectionIndex = withoutTitle.search(/^##\s+/m);
  if (nextSectionIndex === -1) return withoutTitle.trim();
  return withoutTitle.slice(0, nextSectionIndex).trim();
}

function extractFirstSnippet(markdown: string): string | null {
  const matched = markdown.match(/```[a-zA-Z0-9_-]*\n([\s\S]*?)```/);
  return matched?.[1]?.trim() || null;
}

function renderInline(text: string): string {
  let output = escapeHtml(text);
  output = output.replace(/`([^`]+)`/g, (_match, code) => `<code>${escapeHtml(code)}</code>`);
  output = output.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  return output;
}

function escapeHtml(text: string): string {
  return text
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');
}

function slugify(value: string): string {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .trim();
}
