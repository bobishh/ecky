import type { SketchDocument, SketchSuggestionRequest } from './tauri/contracts';
import { sourceLineCount } from './sketchWorkspaceState';

export const EMPTY_SKETCH_DOCUMENT_SOURCE_ERROR = 'Sketch document unavailable.';

export type SketchDocumentSourceInput = SketchDocument | SketchSuggestionRequest | null | undefined;

export type SketchDocumentSourceSummaryRow = {
  id: string;
  label: string;
  value: string;
};

export type SketchDocumentSourceSummary = {
  summary: string;
  rows: SketchDocumentSourceSummaryRow[];
  lineCount: number;
  primitiveCount: number;
  error: string | null;
};

export function formatSketchDocumentSource(input: SketchDocumentSourceInput): string {
  if (!input) return '';
  return JSON.stringify(input, null, 2);
}

export function sketchDocumentSummary(input: SketchDocumentSourceInput): SketchDocumentSourceSummary {
  const document = sketchDocumentFromSource(input);
  if (!document) {
    return {
      summary: EMPTY_SKETCH_DOCUMENT_SOURCE_ERROR,
      rows: [],
      lineCount: 0,
      primitiveCount: 0,
      error: EMPTY_SKETCH_DOCUMENT_SOURCE_ERROR,
    };
  }

  const lineCount = sourceLineCount(formatSketchDocumentSource(input));
  const sketchCount = document.sketches?.length ?? 0;
  const primitiveCount =
    document.sketches?.reduce((count, sketch) => count + (sketch.primitives?.length ?? 0), 0) ?? 0;

  return {
    summary: `${formatCount(lineCount, 'line')} / ${formatCount(sketchCount, 'sketch')} / ${formatCount(
      primitiveCount,
      'primitive',
    )}`,
    rows: [
      { id: 'documentId', label: 'Document ID', value: document.documentId },
      { id: 'activeSketchId', label: 'Active sketch', value: document.activeSketchId ?? 'none' },
      { id: 'lineCount', label: 'JSON lines', value: formatCount(lineCount, 'line') },
      { id: 'primitiveCount', label: 'Primitives', value: formatCount(primitiveCount, 'primitive') },
    ],
    lineCount,
    primitiveCount,
    error: null,
  };
}

function sketchDocumentFromSource(input: SketchDocumentSourceInput): SketchDocument | null {
  if (!input) return null;
  if ('document' in input) return input.document;
  return input;
}

function formatCount(count: number, noun: string): string {
  const plural = noun === 'sketch' ? 'sketches' : `${noun}s`;
  return `${count} ${count === 1 ? noun : plural}`;
}
