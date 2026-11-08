import { invoke } from '@tauri-apps/api/core';
import type { LivePaperPrimitive, LivePaperProfile, LivePaperSnapshot } from './tauri/contracts';
export type { LivePaperPrimitive, LivePaperProfile, LivePaperSnapshot } from './tauri/contracts';

export type LivePaperPoint = Extract<LivePaperPrimitive, { kind: 'point' }>['positionMm'];

export function getLivePaper(): Promise<LivePaperSnapshot | null> {
  return invoke('get_live_paper');
}

export function prepareLivePaperProfile(sessionId: string, scopeKey: string): Promise<LivePaperProfile> {
  return invoke('prepare_live_paper_profile', { sessionId, scopeKey });
}

export function livePaperCoordinates(primitive: LivePaperPrimitive): LivePaperPoint[] {
  if (primitive.kind === 'point') return [primitive.positionMm];
  if (primitive.kind === 'line') return [primitive.startMm, primitive.endMm];
  if (primitive.kind === 'circle') return [primitive.centerMm];
  return primitive.pointsMm;
}

export function editLivePaperCoordinate(
  primitive: LivePaperPrimitive, pointIndex: number, axis: 0 | 1, value: number,
): LivePaperPrimitive {
  const next = structuredClone(primitive);
  const point = livePaperCoordinates(next)[pointIndex];
  if (!point || !Number.isFinite(value)) throw new Error('Coordinate must be a finite number.');
  point[axis] = value;
  return next;
}

export function livePaperStateLabel(state: LivePaperSnapshot['state']): string {
  const labels: Record<string, string> = {
    waiting: 'Waiting for phone', connecting: 'Connecting camera', streaming: 'Live paper',
    markerLost: 'Markers lost · updates paused', stopped: 'Capture stopped', error: 'Camera needs attention',
  };
  return labels[state] ?? state;
}

/** Bind capture to the exact model scope without broadcasting its source in frame events. */
export async function livePaperScopeToken(sourceScope: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(sourceScope));
  return `sha256:${Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('')}`;
}
