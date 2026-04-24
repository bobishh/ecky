import type { SketchPreviewHullRequest } from './tauri/contracts';
import { buildSketchSuggestionDocument } from './sketchSuggestionDocument';
import { buildSketchDraftRequest, type SketchStroke } from './sketchWorkspaceState';

export type SketchPreviewHullRequestResult = SketchPreviewHullRequest | { error: string };

export function shouldUseSketchPreviewHull(strokes: SketchStroke[]): boolean {
  const document = buildSketchSuggestionDocument(strokes);
  if (!document?.sketches?.length) return false;

  const views = new Set(document.sketches.map((sketch) => sketch.view));
  return views.has('front') && (views.has('top') || views.has('side'));
}

export function buildSketchPreviewHullRequest(strokes: SketchStroke[]): SketchPreviewHullRequestResult {
  const draftRequest = buildSketchDraftRequest(strokes);
  if ('error' in draftRequest) return draftRequest;

  const document = buildSketchSuggestionDocument(strokes);
  if (!document) return { error: 'Preview hull requires closed Front plus Top or Side profile.' };

  const views = new Set(document.sketches?.map((sketch) => sketch.view) ?? []);
  if (!views.has('front') || (!views.has('top') && !views.has('side'))) {
    return { error: 'Preview hull requires closed Front plus Top or Side profile.' };
  }

  return {
    partId: 'sketch-preview-hull',
    document,
    fallbackDepth: draftRequest.amount,
  };
}
