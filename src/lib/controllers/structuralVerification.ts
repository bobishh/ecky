/**
 * Pure logic for structural verification of generated model bundles.
 *
 * Kept framework-free so it can be unit-tested without Tauri or Svelte.
 */

import type {
  AuthoredVerifyCheck,
  AuthoredVerifyCheckStatus,
  AuthoredVerifyValue,
  StructuralVerificationResult,
  StructuralIssue,
  StructuralMetrics,
} from '../types/domain';

export type StructuralVerifyFn = (
  modelId: string,
  originalPrompt: string,
) => Promise<StructuralVerificationResult>;

export type StructuralCheckResult =
  | {
      kind: 'structural_passed';
      metrics: StructuralMetrics;
      verification: StructuralVerificationResult;
    }
  | { kind: 'structural_skipped'; reason: string }
  | { kind: 'failed_terminal'; issues: string; verification: StructuralVerificationResult }
  | { kind: 'repair_needed'; repairPrompt: string; verification: StructuralVerificationResult };

export type AuthoredVerifyChipTone = 'green' | 'red';

export type AuthoredVerifyChip = {
  id: string;
  label: string;
  status: AuthoredVerifyCheckStatus;
  tone: AuthoredVerifyChipTone;
  message: string;
  stableNodeId: string | null;
};

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
    return { kind: 'structural_passed', metrics: result.metrics, verification: result };
  }

  const authoredVerifyFailed = hasAuthoredVerifyIssues(result);

  // Build a compact summary of structural issues for repair/terminal
  const issueLines = result.issues
    .map((i) => `- [${i.code}] ${i.message}`)
    .join('\n');
  const issuesSummary = [
    ...(authoredVerifyFailed ? ['Authored verify requirements failed.'] : []),
    result.summary,
    '',
    'Issues:',
    issueLines,
  ].join('\n');

  const hasMoreAttempts = opts.currentGenerationAttempt < opts.maxGenerationAttempts;

  if (hasMoreAttempts) {
    const metricsBlock = formatMetricsBlock(result.metrics);
    const authoredVerifyFeedback = formatAuthoredVerifyFeedback(result.issues);
    const repairPrompt = [
      ...(authoredVerifyFailed ? ['Authored verify requirements failed.'] : []),
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
      ...(authoredVerifyFeedback.length > 0
        ? ['Authored verify retry feedback:', ...authoredVerifyFeedback, '']
        : []),
      ...(authoredVerifyFailed
        ? [
            'Authored verify guidance:',
            '- Do not remove or weaken `(verify ...)` clauses.',
            '- Change geometry, topology, or required exports so authored checks pass.',
            '',
          ]
        : []),
      ...(metricsBlock ? [`Metrics:`, metricsBlock, ``] : []),
      `Original request: ${opts.originalPrompt}`,
      ``,
      `Please fix the geometry code to resolve the structural issues.`,
    ].join('\n');

    return { kind: 'repair_needed', repairPrompt, verification: result };
  }

  return { kind: 'failed_terminal', issues: issuesSummary, verification: result };
}

export function deriveAuthoredVerifyChips(
  result: StructuralVerificationResult | null | undefined,
): AuthoredVerifyChip[] {
  const checks = result?.authoredVerifyChecks ?? [];
  return checks.map((check) => {
    const label = normalizeAuthoredVerifyTag(check);
    const stableNodeId = check.stableNodeId ?? null;
    return {
      id: stableNodeId ?? `authored-verify:${label}`,
      label,
      status: check.status,
      tone: check.status === 'passed' ? 'green' : 'red',
      message: formatAuthoredVerifyChipMessage(check),
      stableNodeId,
    };
  });
}

function normalizeAuthoredVerifyTag(check: AuthoredVerifyCheck): string {
  const tag = `${check.tag ?? ''}`.trim();
  return tag || 'verify';
}

function formatAuthoredVerifyChipMessage(check: AuthoredVerifyCheck): string {
  const expected = formatAuthoredVerifyValue(check.expected);
  const actual = formatAuthoredVerifyValue(check.actual);
  const comparator = `${check.comparator ?? ''}`.trim();

  if (!expected || !actual || !comparator) {
    return check.message;
  }

  const metricParts = [`${check.metricSource ?? ''}`.trim(), `${check.metricKey ?? ''}`.trim()]
    .filter((part) => part.length > 0);
  const prefix = metricParts.length > 0 ? `${metricParts.join(' ')} ` : '';
  return `${prefix}expected ${comparator} ${expected}; actual ${actual}`;
}

function formatAuthoredVerifyValue(value: AuthoredVerifyValue | null | undefined): string | null {
  if (!value) return null;
  switch (value.kind) {
    case 'number':
      return Number.isFinite(value.value) ? `${value.value}` : null;
    case 'boolean':
      return value.value ? 'true' : 'false';
    case 'text':
      return value.value;
    default:
      return null;
  }
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

function formatAuthoredVerifyFeedback(issues: StructuralIssue[]): string[] {
  return issues
    .filter((issue) => issue.code.startsWith('AUTHORED_VERIFY_'))
    .map((issue) => {
      const tag = issue.partId ?? 'model';
      const value = issue.numericPayload != null ? ` (value: ${issue.numericPayload})` : '';
      return `- verify ${tag}: ${issue.message}${value}`;
    });
}

function hasAuthoredVerifyIssues(result: StructuralVerificationResult): boolean {
  return result.issues.some((issue) => issue.code.startsWith('AUTHORED_VERIFY_'));
}
