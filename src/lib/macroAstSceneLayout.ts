import type { MacroAstMapNode, MacroAstMapProjection } from './macroAstMap';

export type MacroAstScenePoint = { x: number; y: number };

export type MacroAstSceneNodeLayout = {
  id: string;
  kind: MacroAstMapNode['kind'];
  label: string;
  syntaxVariant?: string;
  syntaxLabel?: string;
  fieldKey?: string;
  x: number;
  y: number;
  w: number;
  h: number;
  controlAnchor: MacroAstScenePoint;
  portAnchors: MacroAstScenePoint[];
  shapePath: string;
};

export type MacroAstSceneConnector = {
  id: string;
  fromId: string;
  toId: string;
  path: string;
};

export type MacroAstSceneLayout = {
  width: number;
  height: number;
  nodes: MacroAstSceneNodeLayout[];
  connectors: MacroAstSceneConnector[];
};

export type MacroAstSceneLayoutHints = {
  width?: number;
  minPartWidth?: number;
  maxColumns?: number;
};

type BalancedColumn = {
  height: number;
};

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function formatBlobPath(x: number, y: number, w: number, h: number): string {
  const rx = Math.max(8, Math.min(w * 0.18, h * 0.22));
  const ry = Math.max(8, Math.min(h * 0.18, w * 0.16));
  const x2 = x + w;
  const y2 = y + h;
  return [
    `M ${x + rx} ${y}`,
    `C ${x + rx * 0.35} ${y} ${x + rx * 0.05} ${y + ry * 0.25} ${x} ${y + ry}`,
    `L ${x} ${y2 - ry}`,
    `C ${x + rx * 0.15} ${y2 - ry * 0.15} ${x + rx * 0.4} ${y2} ${x + rx} ${y2}`,
    `L ${x2 - rx} ${y2}`,
    `C ${x2 - rx * 0.25} ${y2} ${x2} ${y2 - ry * 0.3} ${x2} ${y2 - ry}`,
    `L ${x2} ${y + ry}`,
    `C ${x2} ${y + ry * 0.2} ${x2 - rx * 0.25} ${y} ${x2 - rx} ${y}`,
    'Z',
  ].join(' ');
}

function formatLocalBlobPath(w: number, h: number): string {
  return formatBlobPath(0, 0, w, h);
}

function connectorPath(from: MacroAstScenePoint, to: MacroAstScenePoint): string {
  const midY = (from.y + to.y) / 2;
  return `M ${from.x} ${from.y} C ${from.x} ${midY} ${to.x} ${midY} ${to.x} ${to.y}`;
}

function createBalancedColumns(count: number): BalancedColumn[] {
  return Array.from({ length: Math.max(1, count) }, () => ({ height: 0 }));
}

function pickShortestColumn(columns: BalancedColumn[]): [BalancedColumn, number] {
  let shortestIndex = 0;
  for (let index = 1; index < columns.length; index += 1) {
    if (columns[index]!.height < columns[shortestIndex]!.height) shortestIndex = index;
  }
  return [columns[shortestIndex]!, shortestIndex];
}

function resolvePartColumns(cellWidth: number, portCount: number): number {
  return clamp(Math.floor(cellWidth / 340), 1, Math.min(3, Math.max(1, portCount)));
}

export function buildMacroAstSceneLayout(
  projection: MacroAstMapProjection,
  hints: MacroAstSceneLayoutHints = {},
): MacroAstSceneLayout {
  const width = hints.width ?? 1120;
  const root = projection.root;
  const parts = root.children || [];
  const columns = clamp(Math.ceil(Math.sqrt(Math.max(1, parts.length))), 1, hints.maxColumns ?? 3);
  const sceneGapX = 20;
  const sceneGapY = 24;
  const usableWidth = width - 32;
  const cellWidth = Math.floor((usableWidth - sceneGapX * (columns - 1)) / columns);
  const minPartWidth = hints.minPartWidth ?? 360;

  const partLayouts = parts.map((part) => {
    const portCount = part.children?.length ?? 0;
    const partColumns = portCount > 0 ? resolvePartColumns(cellWidth, portCount) : 1;
    const portColumnGap = 14;
    const preferredPortColumnWidth = clamp(Math.floor(cellWidth / 4), 220, 300);
    const contentWidth =
      28 + partColumns * preferredPortColumnWidth + portColumnGap * Math.max(0, partColumns - 1);
    const partW = clamp(contentWidth, Math.min(minPartWidth, cellWidth), cellWidth);
    const portColumnWidth = Math.max(
      180,
      Math.floor((partW - 28 - portColumnGap * Math.max(0, partColumns - 1)) / partColumns),
    );
    const partColumnHeights = createBalancedColumns(partColumns);
    for (const port of part.children || []) {
      const [, columnIndex] = pickShortestColumn(partColumnHeights);
      const paramPresent = Boolean(port.children?.[0]);
      const moduleHeight = 52 + (paramPresent ? 44 + 54 : 0) + 12;
      partColumnHeights[columnIndex]!.height += moduleHeight;
    }
    const height = 128 + Math.max(...partColumnHeights.map((column) => column.height), 0);
    return {
      width: partW,
      height,
      portColumns: partColumns,
      portColumnGap,
      portColumnWidth,
    };
  });

  const rootW = clamp(
    Math.max(
      minPartWidth + 160,
      (partLayouts.length > 0 ? Math.max(...partLayouts.map((layout) => layout.width)) : minPartWidth) + 72,
    ),
    Math.min(width - 32, 760),
    width - 32,
  );
  const rootH = 96;
  const rootX = Math.round((width - rootW) / 2);
  const rootY = 16;
  const nodes: MacroAstSceneNodeLayout[] = [];
  const connectors: MacroAstSceneConnector[] = [];

  nodes.push({
    id: root.id,
    kind: root.kind,
    label: root.label,
    syntaxVariant: root.syntaxVariant,
    syntaxLabel: root.syntaxLabel,
    x: rootX,
    y: rootY,
    w: rootW,
    h: rootH,
    controlAnchor: { x: rootX + rootW / 2, y: rootY + rootH - 8 },
    portAnchors: [],
    shapePath: formatLocalBlobPath(rootW, rootH),
  });

  const rowHeights: number[] = [];
  for (let i = 0; i < parts.length; i += columns) {
    rowHeights.push(Math.max(...partLayouts.slice(i, i + columns).map((layout) => layout.height)));
  }

  const rootAnchor = nodes[0]!.controlAnchor;
  const partYStart = rootY + rootH + 28;

  let partIndex = 0;
  let currentY = partYStart;
  for (let rowIndex = 0; partIndex < parts.length; rowIndex += 1) {
    const rowParts = parts.slice(partIndex, partIndex + columns);
    const rowHeight = rowHeights[rowIndex] ?? 0;

    for (let columnIndex = 0; columnIndex < rowParts.length; columnIndex += 1) {
      const part = rowParts[columnIndex]!;
      const layout = partLayouts[partIndex + columnIndex]!;
      const x = 16 + columnIndex * (cellWidth + sceneGapX) + Math.floor((cellWidth - layout.width) / 2);
      const y = currentY;
      const w = layout.width;
      const h = layout.height;

      nodes.push({
        id: part.id,
        kind: part.kind,
        label: part.label,
        syntaxVariant: part.syntaxVariant,
        syntaxLabel: part.syntaxLabel,
        x,
        y,
        w,
        h,
        controlAnchor: { x: x + w / 2, y: y + 20 },
        portAnchors: [],
        shapePath: formatLocalBlobPath(w, h),
      });
      connectors.push({
        id: `${root.id}->${part.id}`,
        fromId: root.id,
        toId: part.id,
        path: connectorPath(rootAnchor, { x: x + w / 2, y }),
      });

      const portCount = part.children?.length ?? 0;
      if (portCount > 0) {
        const portColumns = layout.portColumns;
        const portColumnGap = layout.portColumnGap;
        const portColumnWidth = layout.portColumnWidth;
        const portColumnsState = createBalancedColumns(portColumns);
        const portStartY = y + 54;

        for (const port of part.children || []) {
          const [shortestColumn, shortestIndex] = pickShortestColumn(portColumnsState);
          const columnX = x + 14 + shortestIndex * (portColumnWidth + portColumnGap);
          const portY = portStartY + shortestColumn.height;
          const portH = 52;
          const param = port.children?.[0] ?? null;
          const paramH = param ? 44 : 0;
          const moduleHeight = portH + (param ? 54 + paramH : 0) + 12;

          const portNode: MacroAstSceneNodeLayout = {
            id: port.id,
            kind: port.kind,
            label: port.label,
            syntaxVariant: port.syntaxVariant,
            syntaxLabel: port.syntaxLabel,
            x: columnX,
            y: portY,
            w: portColumnWidth,
            h: portH,
            controlAnchor: { x: columnX + portColumnWidth - 20, y: portY + portH / 2 },
            portAnchors: [],
            shapePath: formatLocalBlobPath(portColumnWidth, portH),
          };
          nodes.push(portNode);
          connectors.push({
            id: `${part.id}->${port.id}`,
            fromId: part.id,
            toId: port.id,
            path: connectorPath({ x: x + w / 2, y: y + 28 }, { x: columnX, y: portY + portH / 2 }),
          });

          if (param) {
            const paramX = columnX + 18;
            const paramY = portY + 54;
            const paramW = Math.max(180, portColumnWidth - 36);
            const paramNode: MacroAstSceneNodeLayout = {
              id: param.id,
              kind: param.kind,
              label: param.label,
              syntaxVariant: param.syntaxVariant,
              syntaxLabel: param.syntaxLabel,
              fieldKey: param.fieldKey,
              x: paramX,
              y: paramY,
              w: paramW,
              h: paramH,
              controlAnchor: { x: paramX + paramW - 20, y: paramY + paramH / 2 },
              portAnchors: [],
              shapePath: formatLocalBlobPath(paramW, paramH),
            };
            nodes.push(paramNode);
            connectors.push({
              id: `${port.id}->${param.id}`,
              fromId: port.id,
              toId: param.id,
              path: connectorPath(portNode.controlAnchor, paramNode.controlAnchor),
            });
          }

          shortestColumn.height += moduleHeight;
        }
      }
    }

    currentY += rowHeight + sceneGapY;
    partIndex += columns;
  }

  const height = Math.max(
    currentY + 28,
    Math.max(...nodes.map((node) => node.y + node.h + 24), rootY + rootH + 24),
  );

  return {
    width,
    height,
    nodes,
    connectors,
  };
}

function macroAstSceneNodeMap(layout: MacroAstSceneLayout): Map<string, MacroAstSceneNodeLayout> {
  return new Map(layout.nodes.map((node) => [node.id, node]));
}

function macroAstSceneNodeById(layout: MacroAstSceneLayout, nodeId: string) {
  return layout.nodes.find((node) => node.id === nodeId) ?? null;
}
