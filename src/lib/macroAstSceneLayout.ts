import type { MacroAstMapNode, MacroAstMapProjection } from './macroAstMap';

export type MacroAstScenePoint = { x: number; y: number };

export type MacroAstSceneNodeLayout = {
  id: string;
  kind: MacroAstMapNode['kind'];
  label: string;
  syntaxVariant?: string;
  syntaxLabel?: string;
  fieldKey?: string;
  value?: MacroAstMapNode['value'];
  sourceRange?: MacroAstMapNode['sourceRange'];
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
  /** Next free grid cell: the "+ add part" ghost slot anchor. */
  insertSlot: { x: number; y: number; w: number; h: number };
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

function resolvePartColumns(cellWidth: number, paramCount: number): number {
  return clamp(Math.floor(cellWidth / 260), 1, Math.min(3, Math.max(1, paramCount)));
}

export function buildMacroAstSceneLayout(
  projection: MacroAstMapProjection,
  hints: MacroAstSceneLayoutHints = {},
): MacroAstSceneLayout {
  const width = hints.width ?? 1120;
  const root = projection.root;
  const parts = root.children || [];
  const columns = clamp(Math.ceil(Math.sqrt(Math.max(1, parts.length))), 1, hints.maxColumns ?? 3);
  const sceneGapX = 16;
  const sceneGapY = 18;
  const usableWidth = width - 24;
  const cellWidth = Math.floor((usableWidth - sceneGapX * (columns - 1)) / columns);
  const minPartWidth = Math.min(hints.minPartWidth ?? 300, cellWidth);

  const partHeaderH = 34;
  const partPadX = 10;
  const partPadBottom = 10;
  const moduleH = 58;
  const moduleGap = 8;

  const partLayouts = parts.map((part) => {
    const paramCount = part.children?.length ?? 0;
    const paramColumns = paramCount > 0 ? resolvePartColumns(cellWidth, paramCount) : 1;
    const paramColumnGap = 10;
    const preferredColumnWidth = clamp(Math.floor(cellWidth / 3), 210, 290);
    const contentWidth =
      partPadX * 2 + paramColumns * preferredColumnWidth + paramColumnGap * Math.max(0, paramColumns - 1);
    const partW = clamp(contentWidth, minPartWidth, cellWidth);
    const paramColumnWidth = Math.max(
      170,
      Math.floor((partW - partPadX * 2 - paramColumnGap * Math.max(0, paramColumns - 1)) / paramColumns),
    );
    const rows = paramColumns > 0 ? Math.ceil(paramCount / paramColumns) : 0;
    const height =
      partHeaderH + (rows > 0 ? rows * (moduleH + moduleGap) : 8) + partPadBottom;
    return {
      width: partW,
      height,
      paramColumns,
      paramColumnGap,
      paramColumnWidth,
    };
  });

  // Root is a slim title bar, not a content region.
  const rootW = usableWidth;
  const rootH = 40;
  const rootX = 12;
  const rootY = 8;
  const nodes: MacroAstSceneNodeLayout[] = [];
  const connectors: MacroAstSceneConnector[] = [];

  nodes.push({
    id: root.id,
    kind: root.kind,
    label: root.label,
    syntaxVariant: root.syntaxVariant,
    syntaxLabel: root.syntaxLabel,
    sourceRange: root.sourceRange,
    x: rootX,
    y: rootY,
    w: rootW,
    h: rootH,
    controlAnchor: { x: rootX + rootW / 2, y: rootY + rootH - 4 },
    portAnchors: [],
    shapePath: formatLocalBlobPath(rootW, rootH),
  });

  const rowHeights: number[] = [];
  for (let i = 0; i < parts.length; i += columns) {
    rowHeights.push(Math.max(...partLayouts.slice(i, i + columns).map((layout) => layout.height)));
  }

  const rootAnchor = nodes[0]!.controlAnchor;
  const partYStart = rootY + rootH + 18;

  let partIndex = 0;
  let currentY = partYStart;
  for (let rowIndex = 0; partIndex < parts.length; rowIndex += 1) {
    const rowParts = parts.slice(partIndex, partIndex + columns);
    const rowHeight = rowHeights[rowIndex] ?? 0;

    for (let columnIndex = 0; columnIndex < rowParts.length; columnIndex += 1) {
      const part = rowParts[columnIndex]!;
      const layout = partLayouts[partIndex + columnIndex]!;
      const x = 12 + columnIndex * (cellWidth + sceneGapX) + Math.floor((cellWidth - layout.width) / 2);
      const y = currentY;
      const w = layout.width;
      const h = layout.height;

      nodes.push({
        id: part.id,
        kind: part.kind,
        label: part.label,
        sourceRange: part.sourceRange,
        syntaxVariant: part.syntaxVariant,
        syntaxLabel: part.syntaxLabel,
        x,
        y,
        w,
        h,
        controlAnchor: { x: x + w / 2, y: y + 16 },
        portAnchors: [],
        shapePath: formatLocalBlobPath(w, h),
      });
      connectors.push({
        id: `${root.id}->${part.id}`,
        fromId: root.id,
        toId: part.id,
        path: connectorPath(rootAnchor, { x: x + w / 2, y }),
      });

      const params = part.children || [];
      if (params.length > 0) {
        const paramColumns = layout.paramColumns;
        const paramColumnGap = layout.paramColumnGap;
        const paramColumnWidth = layout.paramColumnWidth;
        const columnsState = createBalancedColumns(paramColumns);
        const paramStartY = y + partHeaderH;

        for (const param of params) {
          const [shortestColumn, shortestIndex] = pickShortestColumn(columnsState);
          const columnX = x + partPadX + shortestIndex * (paramColumnWidth + paramColumnGap);
          const paramY = paramStartY + shortestColumn.height;

          nodes.push({
            id: param.id,
            kind: param.kind,
            label: param.label,
            syntaxVariant: param.syntaxVariant,
            syntaxLabel: param.syntaxLabel,
            fieldKey: param.fieldKey,
            value: param.value,
            x: columnX,
            y: paramY,
            w: paramColumnWidth,
            h: moduleH,
            controlAnchor: { x: columnX + paramColumnWidth - 14, y: paramY + moduleH / 2 },
            // Port dot on the left edge: the input enters the module here.
            portAnchors: [{ x: columnX, y: paramY + moduleH / 2 }],
            shapePath: formatLocalBlobPath(paramColumnWidth, moduleH),
          });

          shortestColumn.height += moduleH + moduleGap;
        }
      }
    }

    currentY += rowHeight + sceneGapY;
    partIndex += columns;
  }

  // Ghost slot: the next cell in the part grid.
  const placedParts = parts.length;
  const slotColumn = placedParts % columns;
  const lastRowY = placedParts === 0 ? partYStart : currentY - (rowHeights[rowHeights.length - 1] ?? 0) - sceneGapY;
  const slotY = slotColumn === 0 && placedParts > 0 ? currentY : placedParts === 0 ? partYStart : lastRowY;
  const slotW = Math.min(cellWidth, Math.max(minPartWidth, 240));
  const insertSlot = {
    x: 12 + slotColumn * (cellWidth + sceneGapX) + Math.floor((cellWidth - slotW) / 2),
    y: slotY,
    w: slotW,
    h: 96,
  };

  const height = Math.max(
    currentY + 16,
    insertSlot.y + insertSlot.h + 16,
    Math.max(...nodes.map((node) => node.y + node.h + 16), rootY + rootH + 16),
  );

  return {
    width,
    height,
    nodes,
    connectors,
    insertSlot,
  };
}

function macroAstSceneNodeMap(layout: MacroAstSceneLayout): Map<string, MacroAstSceneNodeLayout> {
  return new Map(layout.nodes.map((node) => [node.id, node]));
}

function macroAstSceneNodeById(layout: MacroAstSceneLayout, nodeId: string) {
  return layout.nodes.find((node) => node.id === nodeId) ?? null;
}
