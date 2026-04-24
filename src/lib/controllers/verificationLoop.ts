/**
 * Pure logic for the vision verification loop.
 *
 * Kept framework-free so it can be unit-tested without Tauri or Svelte.
 */

import type { StructuralVerificationResult, VisualVerificationResult, StructuralMetrics } from '../types/domain';
import { runStructuralCheck } from './structuralVerification';
import type { StructuralVerifyFn } from './structuralVerification';

export type VerifyFn = (
  prompt: string,
  screenshots: string[],
  referenceImages: string[],
  structuralSummary: string | null,
) => Promise<VisualVerificationResult>;

export type CaptureFn = () => string[];

export type VerificationLoopResult =
  | { kind: 'passed' }
  | { kind: 'skipped'; reason: string }
  | { kind: 'failed_terminal'; issues: string }
  | { kind: 'repair_needed'; repairPrompt: string };

export interface VerificationLoopOptions {
  originalPrompt: string;
  maxVerifyAttempts: number;
  currentGenerationAttempt: number;
  maxGenerationAttempts: number;
  /** Explicit reason to skip screenshot verification before capture. */
  skipReason?: string | null;
  capture: CaptureFn;
  verify: VerifyFn;
  /** Reference images from user attachments (data URLs). */
  referenceImages?: string[];
  /** Structural summary from stage 1 to provide context. */
  structuralSummary?: string | null;
  /** Structural metrics from stage 1 for the repair prompt. */
  structuralMetrics?: StructuralMetrics | null;
}

export interface TwoStageOptions {
  modelId: string;
  originalPrompt: string;
  maxVerifyAttempts: number;
  currentGenerationAttempt: number;
  maxGenerationAttempts: number;
  /** Explicit reason to skip screenshot verification before capture. */
  screenshotSkipReason?: string | null;
  capture: CaptureFn;
  verifyScreenshot: VerifyFn;
  verifyStructural: StructuralVerifyFn;
  /** Reference images from user attachments (data URLs). */
  referenceImages?: string[];
}

/**
 * Run one round of vision verification.
 *
 * Returns what the caller (orchestrator) should do next:
 * - `passed`          → commit the result
 * - `skipped`         → verification disabled or viewer unavailable, commit anyway
 * - `failed_terminal` → all verify attempts used up, commit anyway (best-effort)
 * - `repair_needed`   → regenerate with repairPrompt
 */
export async function runVerificationRound(
  verifyAttempt: number,
  opts: VerificationLoopOptions,
): Promise<VerificationLoopResult> {
  if (opts.maxVerifyAttempts <= 0) {
    return { kind: 'skipped', reason: 'verification disabled' };
  }

  const skipReason = `${opts.skipReason ?? ''}`.trim();
  if (skipReason) {
    return { kind: 'skipped', reason: skipReason };
  }

  const screenshots = opts.capture();
  if (screenshots.length === 0) {
    return { kind: 'skipped', reason: 'viewer not ready' };
  }

  let result: VisualVerificationResult;
  try {
    result = await opts.verify(
      opts.originalPrompt,
      screenshots,
      opts.referenceImages ?? [],
      opts.structuralSummary ?? null,
    );
  } catch {
    return { kind: 'skipped', reason: 'verify call failed' };
  }

  if (result.passed) {
    return { kind: 'passed' };
  }

  const hasMoreVerifyAttempts = verifyAttempt < opts.maxVerifyAttempts;
  const hasMoreGenerationAttempts =
    opts.currentGenerationAttempt < opts.maxGenerationAttempts;

  if (hasMoreVerifyAttempts && hasMoreGenerationAttempts) {
    return {
      kind: 'repair_needed',
      repairPrompt: buildVisualRepairPrompt(result, opts),
    };
  }

  // Out of attempts — commit anyway (best-effort, non-blocking)
  return { kind: 'failed_terminal', issues: formatVisualIssues(result) };
}

/**
 * Two-stage verification: structural first, then screenshot.
 *
 * - Structural fail → repair/terminal (no screenshots)
 * - Structural pass → proceed to screenshot verification
 * - Structural skipped → fall through to screenshot verification
 */
export async function runTwoStageVerification(
  verifyAttempt: number,
  opts: TwoStageOptions,
): Promise<VerificationLoopResult> {
  // Stage 1: Structural verification
  const structural = await runStructuralCheck({
    modelId: opts.modelId,
    originalPrompt: opts.originalPrompt,
    currentGenerationAttempt: opts.currentGenerationAttempt,
    maxGenerationAttempts: opts.maxGenerationAttempts,
    verify: opts.verifyStructural,
  });

  let structuralSummary: string | null = null;
  let structuralMetrics: StructuralMetrics | null = null;

  switch (structural.kind) {
    case 'repair_needed':
      return { kind: 'repair_needed', repairPrompt: structural.repairPrompt };
    case 'failed_terminal':
      return { kind: 'failed_terminal', issues: structural.issues };
    case 'structural_passed':
      structuralMetrics = structural.metrics;
      structuralSummary = formatStructuralSummaryForVisual(structural.metrics);
      break;
    case 'structural_skipped':
      break;
  }

  // Stage 2: Screenshot verification
  return runVerificationRound(verifyAttempt, {
    originalPrompt: opts.originalPrompt,
    maxVerifyAttempts: opts.maxVerifyAttempts,
    currentGenerationAttempt: opts.currentGenerationAttempt,
    maxGenerationAttempts: opts.maxGenerationAttempts,
    skipReason: opts.screenshotSkipReason ?? null,
    capture: opts.capture,
    verify: opts.verifyScreenshot,
    referenceImages: opts.referenceImages,
    structuralSummary,
    structuralMetrics,
  });
}

// ── Helpers ─────────────────────────────────────────────────────────────────

function formatVisualIssues(result: VisualVerificationResult): string {
  if (result.issues.length === 0) return result.summary;
  return result.issues
    .map((issue) => {
      const label = issue.partLabel ? ` (${issue.partLabel})` : '';
      return `[${issue.category}] ${issue.description}${label}`;
    })
    .join('; ');
}

function buildVisualRepairPrompt(
  result: VisualVerificationResult,
  opts: VerificationLoopOptions,
): string {
  const sections: string[] = [];

  sections.push('Visual verification failed.');

  // Structural context if available
  if (opts.structuralMetrics) {
    const m = opts.structuralMetrics;
    const metricLines = [`  parts: ${m.partCount}`];
    if (m.totalVolume != null) metricLines.push(`  volume: ${m.totalVolume.toFixed(2)}`);
    if (m.totalArea != null) metricLines.push(`  area: ${m.totalArea.toFixed(2)}`);
    sections.push(`Structural context:\n${metricLines.join('\n')}`);
  }

  // Structured issue list
  if (result.issues.length > 0) {
    sections.push('Issues:');
    for (const issue of result.issues) {
      const label = issue.partLabel ? ` [part: ${issue.partLabel}]` : '';
      sections.push(`- [${issue.category}] ${issue.description}${label}`);
    }
  } else {
    sections.push(`Summary: ${result.summary}`);
  }

  // Reference expectations
  if (opts.referenceImages && opts.referenceImages.length > 0) {
    const refMismatches = result.issues.filter((i) => i.category === 'reference_mismatch');
    if (refMismatches.length > 0) {
      sections.push('Reference image mismatches:');
      for (const m of refMismatches) {
        sections.push(`- ${m.description}`);
      }
    }
  }

  sections.push(`\nOriginal request: ${opts.originalPrompt}`);
  sections.push('Please fix the geometry to resolve the visual issues listed above.');

  return sections.join('\n');
}

function formatStructuralSummaryForVisual(metrics: StructuralMetrics): string {
  const lines = [
    `Structural checks passed.`,
    `Parts: ${metrics.partCount}`,
  ];
  if (metrics.totalVolume != null) lines.push(`Volume: ${metrics.totalVolume.toFixed(2)}mm³`);
  if (metrics.totalArea != null) lines.push(`Area: ${metrics.totalArea.toFixed(2)}mm²`);
  if (metrics.bbox) {
    const b = metrics.bbox;
    lines.push(`BBox: [${b.xMin.toFixed(1)}, ${b.yMin.toFixed(1)}, ${b.zMin.toFixed(1)}] → [${b.xMax.toFixed(1)}, ${b.yMax.toFixed(1)}, ${b.zMax.toFixed(1)}]`);
  }
  return lines.join('\n');
}
