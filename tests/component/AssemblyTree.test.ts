import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import AssemblyTree from '../../src/lib/components/AssemblyTree.svelte';
import type { AssemblyNode } from '../../src/lib/types/domain';

const sampleTree: AssemblyNode[] = [
  {
    id: 'group:arm',
    name: 'arm',
    partKey: null,
    bounds: null,
    parameterKeys: ['arm_len', 'arm_wall'],
    children: [
      {
        id: 'part:arm_base',
        name: 'arm base',
        partKey: 'arm_base',
        bounds: null,
        parameterKeys: ['arm_len'],
        children: [],
      },
      {
        id: 'part:arm_tip',
        name: 'arm tip',
        partKey: 'arm_tip',
        bounds: null,
        parameterKeys: ['arm_wall'],
        children: [],
      },
    ],
  },
  {
    id: 'part:bracket',
    name: 'bracket',
    partKey: 'bracket',
    bounds: null,
    parameterKeys: ['mount_hole'],
    children: [],
  },
];

describe('AssemblyTree', () => {
  it('renders nested assembly nodes and collapses/expands sub-assemblies', async () => {
    const { getByRole, queryByText, getByText } = render(AssemblyTree, {
      props: { nodes: sampleTree },
    });

    expect(getByText('arm base')).toBeTruthy();
    expect(getByText('arm tip')).toBeTruthy();
    expect(getByText('bracket')).toBeTruthy();

    await fireEvent.click(getByRole('button', { name: 'Collapse arm' }));
    expect(queryByText('arm base')).toBeNull();
    expect(queryByText('arm tip')).toBeNull();

    await fireEvent.click(getByRole('button', { name: 'Expand arm' }));
    expect(getByText('arm base')).toBeTruthy();
  });

  it('invokes select, solo isolate, group hide, ghost toggle, and show-all reset callbacks', async () => {
    const onSelectPart = vi.fn();
    const onToggleIsolatePart = vi.fn();
    const onToggleHidePart = vi.fn();
    const onToggleGhostIsolatedSiblings = vi.fn();
    const onResetVisibility = vi.fn();

    const { getByRole, getByText } = render(AssemblyTree, {
      props: {
        nodes: sampleTree,
        isolatedPartId: 'arm_base',
        ghostIsolatedSiblings: true,
        onSelectPart,
        onToggleIsolatePart,
        onToggleHidePart,
        onToggleGhostIsolatedSiblings,
        onResetVisibility,
      },
    });

    await fireEvent.click(getByText('bracket'));
    expect(onSelectPart).toHaveBeenCalledWith('bracket');

    await fireEvent.click(getByRole('button', { name: 'Isolate bracket' }));
    expect(onToggleIsolatePart).toHaveBeenCalledWith('bracket');

    await fireEvent.click(getByRole('button', { name: 'Hide arm' }));
    expect(onToggleHidePart).toHaveBeenCalledWith(['arm_base', 'arm_tip']);

    await fireEvent.click(getByRole('button', { name: 'GHOST' }));
    expect(onToggleGhostIsolatedSiblings).toHaveBeenCalledTimes(1);

    await fireEvent.click(getByRole('button', { name: 'SHOW ALL' }));
    expect(onResetVisibility).toHaveBeenCalledTimes(1);
  });
});
