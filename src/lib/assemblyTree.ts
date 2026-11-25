import type { AssemblyNode, ManifestBounds, ModelManifest, PartBinding } from './types/domain';

function mergeBounds(
  left: ManifestBounds | null | undefined,
  right: ManifestBounds | null | undefined,
): ManifestBounds | null {
  if (left && right) {
    return {
      xMin: Math.min(left.xMin, right.xMin),
      yMin: Math.min(left.yMin, right.yMin),
      zMin: Math.min(left.zMin, right.zMin),
      xMax: Math.max(left.xMax, right.xMax),
      yMax: Math.max(left.yMax, right.yMax),
      zMax: Math.max(left.zMax, right.zMax),
    };
  }
  return left ?? right ?? null;
}

function splitGroupPrefix(partId: string): [string, string] | null {
  for (const sep of ['/', '::', '__', '.']) {
    const idx = partId.indexOf(sep);
    if (idx > 0 && idx + sep.length < partId.length) {
      const prefix = partId.slice(0, idx).trim();
      const rest = partId.slice(idx + sep.length).trim();
      if (prefix && rest) {
        return [prefix, rest];
      }
    }
  }
  return null;
}

export function deriveAssemblyTreeFromParts(parts: PartBinding[]): AssemblyNode[] {
  if (!parts.length) return [];

  const groupCounts = new Map<string, number>();
  const partPrefixes: Array<string | null> = parts.map((part) => {
    const split = splitGroupPrefix(part.partId);
    if (split) {
      groupCounts.set(split[0], (groupCounts.get(split[0]) ?? 0) + 1);
      return split[0];
    }
    return null;
  });

  const roots: AssemblyNode[] = [];
  const groupIndices = new Map<string, number>();

  for (let index = 0; index < parts.length; index += 1) {
    const part = parts[index];
    const prefix = partPrefixes[index];
    const leaf: AssemblyNode = {
      id: `part:${part.partId}`,
      name: part.label?.trim() || part.partId,
      partKey: part.partId,
      parameterKeys: [...(part.parameterKeys ?? [])],
      children: [],
      bounds: part.bounds ?? null,
    };

    if (prefix && (groupCounts.get(prefix) ?? 0) > 1) {
      let groupIdx = groupIndices.get(prefix);
      if (groupIdx === undefined) {
        groupIdx = roots.length;
        roots.push({
          id: `group:${prefix}`,
          name: prefix.replace(/[_-]+/g, ' '),
          partKey: null,
          parameterKeys: [],
          children: [],
          bounds: null,
        });
        groupIndices.set(prefix, groupIdx);
      }
      const parent = roots[groupIdx];
      parent.bounds = mergeBounds(parent.bounds, leaf.bounds);
      const existingKeys = new Set(parent.parameterKeys ?? []);
      for (const key of leaf.parameterKeys ?? []) {
        if (!existingKeys.has(key)) {
          existingKeys.add(key);
          (parent.parameterKeys ??= []).push(key);
        }
      }
      (parent.children ??= []).push(leaf);
    } else {
      roots.push(leaf);
    }
  }

  return roots;
}

export function resolveAssemblyTree(manifest: ModelManifest | null | undefined): AssemblyNode[] {
  if (!manifest) return [];
  const existing = (manifest as ModelManifest & { assemblyTree?: AssemblyNode[] }).assemblyTree;
  if (Array.isArray(existing) && existing.length > 0) {
    return existing;
  }
  return deriveAssemblyTreeFromParts(manifest.parts ?? []);
}

export function collectNodePartIds(node: AssemblyNode): string[] {
  const result: string[] = [];
  const visit = (current: AssemblyNode) => {
    if (current.partKey) {
      result.push(current.partKey);
    }
    for (const child of current.children ?? []) {
      visit(child);
    }
  };
  visit(node);
  return result;
}

export function findAssemblyNodeByPartId(
  nodes: AssemblyNode[],
  partId: string | null | undefined,
): AssemblyNode | null {
  if (!partId) return null;
  for (const node of nodes) {
    if (node.partKey === partId) return node;
    const nested = findAssemblyNodeByPartId(node.children ?? [], partId);
    if (nested) return nested;
  }
  return null;
}

export function collectScopedParameterKeys(
  manifest: ModelManifest | null | undefined,
  partIds: Iterable<string>,
): Set<string> {
  const targetIds = new Set(partIds);
  const keys = new Set<string>();
  if (!manifest || targetIds.size === 0) return keys;

  for (const part of manifest.parts ?? []) {
    if (targetIds.has(part.partId)) {
      for (const key of part.parameterKeys ?? []) {
        keys.add(key);
      }
    }
  }
  for (const group of manifest.parameterGroups ?? []) {
    if ((group.partIds ?? []).some((id) => targetIds.has(id))) {
      for (const key of group.parameterKeys ?? []) {
        keys.add(key);
      }
    }
  }
  return keys;
}
