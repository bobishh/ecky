export type TopologyMode = 'off' | 'feature' | 'mesh';

const TOPOLOGY_SEQUENCE: TopologyMode[] = ['off', 'feature', 'mesh'];

export function cycleTopologyMode(current: TopologyMode): TopologyMode {
  const index = TOPOLOGY_SEQUENCE.indexOf(current);
  return TOPOLOGY_SEQUENCE[(index + 1) % TOPOLOGY_SEQUENCE.length] ?? 'off';
}

export function topologyModeLabel(current: TopologyMode): string {
  switch (current) {
    case 'feature':
      return 'TOPOLOGY: FEATURE';
    case 'mesh':
      return 'TOPOLOGY: MESH';
    default:
      return 'TOPOLOGY: OFF';
  }
}

export function meshTopologyVisible(current: TopologyMode, partActive: boolean): boolean {
  return current === 'mesh' && partActive;
}

export function meshTopologyOpacity(current: TopologyMode, partActive: boolean): number {
  return meshTopologyVisible(current, partActive) ? 0.28 : 0;
}
