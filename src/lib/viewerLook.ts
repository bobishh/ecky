import type { PartBinding } from './types/domain';

export type ViewerTone = {
  color: number;
  hoverColor: number;
  measuredColor: number;
  emissive: number;
  hoverEmissive: number;
  measuredEmissive: number;
  edge: number;
  topology: number;
};

const BASE_TONES: ViewerTone[] = [
  {
    color: 0xd2bf89,
    hoverColor: 0xdbcb94,
    measuredColor: 0xd8d5aa,
    emissive: 0x5b4120,
    hoverEmissive: 0x0f5146,
    measuredEmissive: 0x1d5e57,
    edge: 0x46341f,
    topology: 0x63dfff,
  },
  {
    color: 0x8ea3ba,
    hoverColor: 0x9bb2c9,
    measuredColor: 0xacc0cf,
    emissive: 0x20344a,
    hoverEmissive: 0x0f5146,
    measuredEmissive: 0x1d5e57,
    edge: 0x2b3e52,
    topology: 0x84ddff,
  },
  {
    color: 0x8cab9a,
    hoverColor: 0x9ec0ad,
    measuredColor: 0xb2cdc0,
    emissive: 0x244036,
    hoverEmissive: 0x0f5146,
    measuredEmissive: 0x1d5e57,
    edge: 0x2f4a40,
    topology: 0x7ce8dc,
  },
  {
    color: 0xb59b86,
    hoverColor: 0xc4ac97,
    measuredColor: 0xd0beb0,
    emissive: 0x4f3628,
    hoverEmissive: 0x0f5146,
    measuredEmissive: 0x1d5e57,
    edge: 0x4a3428,
    topology: 0x9edcff,
  },
];

const SHELL_TONE = BASE_TONES[0];
const HARDWARE_TONE = BASE_TONES[1];
const COVER_TONE = BASE_TONES[2];
const DETAIL_TONE = BASE_TONES[3];

function stableHash(input: string): number {
  let hash = 2166136261;
  for (let index = 0; index < input.length; index += 1) {
    hash ^= input.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}

function bboxVolume(part: PartBinding): number | null {
  const bounds = part.bounds;
  if (!bounds) return null;
  const x = Math.max(0, bounds.xMax - bounds.xMin);
  const y = Math.max(0, bounds.yMax - bounds.yMin);
  const z = Math.max(0, bounds.zMax - bounds.zMin);
  const value = x * y * z;
  return Number.isFinite(value) && value > 0 ? value : null;
}

function densityHint(part: PartBinding): number | null {
  if (typeof part.volume !== 'number' || part.volume <= 0) return null;
  const boxVolume = bboxVolume(part);
  if (!boxVolume || boxVolume <= 0) return null;
  return part.volume / boxVolume;
}

function classifyTone(part: PartBinding): ViewerTone | null {
  const signature = `${part.label} ${part.semanticRole ?? ''} ${part.kind} ${part.partId}`.toLowerCase();
  if (/\b(clamp|bracket|mount|connector|hinge|ring|fastener|bolt|screw|hook|clip)\b/.test(signature)) {
    return HARDWARE_TONE;
  }
  if (/\b(lid|cover|cap|top|insert|panel|door)\b/.test(signature)) {
    return COVER_TONE;
  }
  if (/\b(handle|grip|lever|knob|tab|foot|spacer)\b/.test(signature)) {
    return DETAIL_TONE;
  }
  if (/\b(basket|bin|shell|body|shade|planter|housing|tray|box|bucket|frame|wall)\b/.test(signature)) {
    return SHELL_TONE;
  }

  const density = densityHint(part);
  if (typeof density === 'number') {
    if (density < 0.2) return SHELL_TONE;
    if (density > 0.55) return HARDWARE_TONE;
  }

  return null;
}

export function resolveViewerTone(
  partId: string | null,
  manifestParts: PartBinding[] = [],
): ViewerTone {
  if (!partId) return BASE_TONES[0];
  const part = manifestParts.find((candidate) => candidate.partId === partId);
  if (!part) return BASE_TONES[0];
  const classified = classifyTone(part);
  if (classified) return classified;
  const fallbackIndex = stableHash(`${part.partId}:${part.label}:${part.kind}`) % BASE_TONES.length;
  return BASE_TONES[fallbackIndex];
}
