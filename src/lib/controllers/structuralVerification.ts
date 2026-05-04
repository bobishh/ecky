/**
 * Pure logic for structural verification of generated model bundles.
 *
 * Kept framework-free so it can be unit-tested without Tauri or Svelte.
 */

import type { StructuralVerificationResult, StructuralMetrics } from '../types/domain';

export type StructuralVerifyFn = (
  modelId: string,
  originalPrompt: string,
) => Promise<StructuralVerificationResult>;

export type StructuralCheckResult =
  | { kind: 'structural_passed'; metrics: StructuralMetrics }
  | { kind: 'structural_skipped'; reason: string }
  | { kind: 'failed_terminal'; issues: string }
  | { kind: 'repair_needed'; repairPrompt: string };

export interface StructuralCheckOptions {
  modelId: string;
  originalPrompt: string;
  currentGenerationAttempt: number;
  maxGenerationAttempts: number;
  verify: StructuralVerifyFn;
}

/**
 * Run structural verification on a generated model bundle.
 *
 * Returns what the caller (orchestrator) should do next:
 * - `structural_passed` → proceed to screenshot verification or commit
 * - `structural_skipped` → verifier unavailable, fall through to screenshot verification
 * - `failed_terminal`    → all generation attempts used, fail with structural findings
 * - `repair_needed`      → regenerate with structured repair prompt
 */
export async function runStructuralCheck(
  opts: StructuralCheckOptions,
): Promise<StructuralCheckResult> {
  let result: StructuralVerificationResult;
  try {
    result = await opts.verify(opts.modelId, opts.originalPrompt);
  } catch {
    return { kind: 'structural_skipped', reason: 'structural verify call failed' };
  }

  if (result.verifierStatus === 'skipped_unavailable' || result.verifierStatus === 'skipped_backend_unavailable') {
    return { kind: 'structural_skipped', reason: 'verifier unavailable' };
  }

  if (result.passed) {
    return { kind: 'structural_passed', metrics: result.metrics };
  }

  // Build a compact summary of structural issues for repair/terminal
  const issueLines = result.issues
    .map((i) => `- [${i.code}] ${i.message}`)
    .join('\n');
  const issuesSummary = `${result.summary}\n\nIssues:\n${issueLines}`;

  const hasMoreAttempts = opts.currentGenerationAttempt < opts.maxGenerationAttempts;

  if (hasMoreAttempts) {
    const metricsBlock = formatMetricsBlock(result.metrics);
    const repairPrompt = [
      `Structural verification failed (source: ${result.verifierSource ?? 'unknown'}):`,
      result.summary,
      ``,
      `Issues:`,
      ...result.issues.map((i) => {
        const partRef = i.partId ? ` [part: ${i.partId}]` : '';
        const metric = i.numericPayload != null ? ` (value: ${i.numericPayload})` : '';
        return `- [${i.code}] ${i.message}${partRef}${metric}`;
      }),
      ``,
      ...(metricsBlock ? [`Metrics:`, metricsBlock, ``] : []),
      `Original request: ${opts.originalPrompt}`,
      ``,
      `Please fix the geometry code to resolve the structural issues.`,
    ].join('\n');

    return { kind: 'repair_needed', repairPrompt };
  }

  return { kind: 'failed_terminal', issues: issuesSummary };
}

function formatMetricsBlock(metrics: StructuralMetrics): string | null {
  const lines: string[] = [];
  lines.push(`  parts: ${metrics.partCount}`);
  if (metrics.totalVolume != null) lines.push(`  volume: ${metrics.totalVolume.toFixed(2)}`);
  if (metrics.totalArea != null) lines.push(`  area: ${metrics.totalArea.toFixed(2)}`);
  if (metrics.previewStlSizeBytes != null)
    lines.push(`  preview STL: ${metrics.previewStlSizeBytes} bytes`);
  if (metrics.previewStlTriangleCount != null)
    lines.push(`  triangles: ${metrics.previewStlTriangleCount}`);
  if (metrics.previewStlComponentCount != null)
    lines.push(`  components: ${metrics.previewStlComponentCount}`);
  if (metrics.previewStlNonManifoldEdgeCount != null)
    lines.push(`  non-manifold edges: ${metrics.previewStlNonManifoldEdgeCount}`);
  if (metrics.previewStlOverhangTriangleCount != null)
    lines.push(`  overhang triangles: ${metrics.previewStlOverhangTriangleCount}`);
  if (metrics.previewStlOverhangRatio != null)
    lines.push(`  overhang ratio: ${metrics.previewStlOverhangRatio.toFixed(3)}`);
  return lines.length > 1 ? lines.join('\n') : null;
}
