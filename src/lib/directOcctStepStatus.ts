import { getStepExportPath } from './exportOptions';
import type { ArtifactBundle, RuntimeCapabilities } from './types/domain';

export type DirectOcctStepStatusTone = 'ready' | 'blocked' | 'pending';

export type DirectOcctStepStatus = {
  label: string;
  status: string;
  detail: string;
  tone: DirectOcctStepStatusTone;
};

function fileName(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return parts.at(-1) ?? path;
}

export function buildDirectOcctStepStatus(
  bundle: ArtifactBundle | null | undefined,
  runtimeCapabilities?: RuntimeCapabilities | null,
): DirectOcctStepStatus | null {
  if (!bundle) return null;
  if (bundle.sourceLanguage !== 'ecky' || bundle.geometryBackend !== 'mesh') return null;

  const stepPath = getStepExportPath(bundle);
  if (stepPath) {
    return {
      label: 'DIRECT OCCT STEP FAST PATH',
      status: 'STEP READY',
      detail: `BRep STEP artifact ready: ${fileName(stepPath)}`,
      tone: 'ready',
    };
  }

  const directOcct = runtimeCapabilities?.directOcct ?? null;
  if (!directOcct) {
    return {
      label: 'DIRECT OCCT STEP FAST PATH',
      status: 'UNKNOWN',
      detail: 'Direct OCCT capability was not probed.',
      tone: 'pending',
    };
  }

  if (!directOcct.available) {
    return {
      label: 'DIRECT OCCT STEP FAST PATH',
      status: 'BLOCKED',
      detail: directOcct.detail || 'Direct OCCT unavailable.',
      tone: 'blocked',
    };
  }

  return {
    label: 'DIRECT OCCT STEP FAST PATH',
    status: 'READY / NO STEP',
    detail: 'Direct OCCT ready; no BRep STEP artifact was produced for this model.',
    tone: 'pending',
  };
}
