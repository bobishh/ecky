export type CodeDiffRowKind = 'context' | 'insert' | 'delete';

export type CodeDiffRow = {
  kind: CodeDiffRowKind;
  oldLineNumber: number | null;
  newLineNumber: number | null;
  oldText: string;
  newText: string;
  hunkIndex: number;
};

export type CodeDiffHunk = {
  index: number;
  oldStartLine: number | null;
  oldEndLine: number | null;
  newStartLine: number | null;
  newEndLine: number | null;
  rows: CodeDiffRow[];
};

export type CodeDiffSummary = {
  oldLineCount: number;
  newLineCount: number;
  unchangedLineCount: number;
  insertedLineCount: number;
  deletedLineCount: number;
  changedLineCount: number;
  hunkCount: number;
  isEmpty: boolean;
};

export type CodeDiffResult = {
  hunks: CodeDiffHunk[];
  rows: CodeDiffRow[];
  summary: CodeDiffSummary;
};

export type CodeDiffOptions = {
  contextLines?: number;
};

type DiffOp =
  | {
      kind: 'equal';
      oldIndex: number;
      newIndex: number;
      oldText: string;
      newText: string;
    }
  | {
      kind: 'delete';
      oldIndex: number;
      oldText: string;
    }
  | {
      kind: 'insert';
      newIndex: number;
      newText: string;
    };

function normalizeLines(text: string): string[] {
  const normalized = text.replace(/\r\n?/g, '\n');
  if (normalized.length === 0) {
    return [];
  }

  const lines = normalized.split('\n');
  if (lines[lines.length - 1] === '') {
    lines.pop();
  }

  return lines;
}

function buildLcsMatrix(oldLines: string[], newLines: string[]): number[][] {
  const rows = oldLines.length + 1;
  const cols = newLines.length + 1;
  const matrix: number[][] = Array.from({ length: rows }, () => Array.from({ length: cols }, () => 0));

  for (let oldIndex = oldLines.length - 1; oldIndex >= 0; oldIndex -= 1) {
    for (let newIndex = newLines.length - 1; newIndex >= 0; newIndex -= 1) {
      if (oldLines[oldIndex] === newLines[newIndex]) {
        matrix[oldIndex][newIndex] = matrix[oldIndex + 1][newIndex + 1] + 1;
      } else {
        matrix[oldIndex][newIndex] = Math.max(matrix[oldIndex + 1][newIndex], matrix[oldIndex][newIndex + 1]);
      }
    }
  }

  return matrix;
}

function buildOperations(oldLines: string[], newLines: string[]): DiffOp[] {
  const matrix = buildLcsMatrix(oldLines, newLines);
  const operations: DiffOp[] = [];
  let oldIndex = 0;
  let newIndex = 0;

  while (oldIndex < oldLines.length && newIndex < newLines.length) {
    if (oldLines[oldIndex] === newLines[newIndex]) {
      operations.push({
        kind: 'equal',
        oldIndex,
        newIndex,
        oldText: oldLines[oldIndex],
        newText: newLines[newIndex],
      });
      oldIndex += 1;
      newIndex += 1;
      continue;
    }

    if (matrix[oldIndex + 1][newIndex] >= matrix[oldIndex][newIndex + 1]) {
      operations.push({
        kind: 'delete',
        oldIndex,
        oldText: oldLines[oldIndex],
      });
      oldIndex += 1;
    } else {
      operations.push({
        kind: 'insert',
        newIndex,
        newText: newLines[newIndex],
      });
      newIndex += 1;
    }
  }

  while (oldIndex < oldLines.length) {
    operations.push({
      kind: 'delete',
      oldIndex,
      oldText: oldLines[oldIndex],
    });
    oldIndex += 1;
  }

  while (newIndex < newLines.length) {
    operations.push({
      kind: 'insert',
      newIndex,
      newText: newLines[newIndex],
    });
    newIndex += 1;
  }

  return operations;
}

function opToRow(op: DiffOp, hunkIndex: number): CodeDiffRow {
  if (op.kind === 'equal') {
    return {
      kind: 'context',
      oldLineNumber: op.oldIndex + 1,
      newLineNumber: op.newIndex + 1,
      oldText: op.oldText,
      newText: op.newText,
      hunkIndex,
    };
  }

  if (op.kind === 'delete') {
    return {
      kind: 'delete',
      oldLineNumber: op.oldIndex + 1,
      newLineNumber: null,
      oldText: op.oldText,
      newText: '',
      hunkIndex,
    };
  }

  return {
    kind: 'insert',
    oldLineNumber: null,
    newLineNumber: op.newIndex + 1,
    oldText: '',
    newText: op.newText,
    hunkIndex,
  };
}

function rowLineBounds(row: CodeDiffRow): { oldStart: number | null; oldEnd: number | null; newStart: number | null; newEnd: number | null } {
  return {
    oldStart: row.oldLineNumber,
    oldEnd: row.oldLineNumber,
    newStart: row.newLineNumber,
    newEnd: row.newLineNumber,
  };
}

export function diffCode(oldText: string, newText: string, options: CodeDiffOptions = {}): CodeDiffResult {
  const contextLines = Math.max(0, options.contextLines ?? 2);
  const oldLines = normalizeLines(oldText);
  const newLines = normalizeLines(newText);
  const operations = buildOperations(oldLines, newLines);
  const changeIndexes: number[] = [];

  for (let index = 0; index < operations.length; index += 1) {
    if (operations[index].kind !== 'equal') {
      changeIndexes.push(index);
    }
  }

  if (changeIndexes.length === 0) {
    return {
      hunks: [],
      rows: [],
      summary: {
        oldLineCount: oldLines.length,
        newLineCount: newLines.length,
        unchangedLineCount: oldLines.length,
        insertedLineCount: 0,
        deletedLineCount: 0,
        changedLineCount: 0,
        hunkCount: 0,
        isEmpty: true,
      },
    };
  }

  const ranges: Array<{ start: number; end: number }> = [];
  let rangeStart = Math.max(0, changeIndexes[0] - contextLines);
  let rangeEnd = Math.min(operations.length - 1, changeIndexes[0] + contextLines);

  for (let i = 1; i < changeIndexes.length; i += 1) {
    const index = changeIndexes[i];
    const nextStart = Math.max(0, index - contextLines);
    const nextEnd = Math.min(operations.length - 1, index + contextLines);

    if (nextStart <= rangeEnd + 1) {
      rangeEnd = Math.max(rangeEnd, nextEnd);
      continue;
    }

    ranges.push({ start: rangeStart, end: rangeEnd });
    rangeStart = nextStart;
    rangeEnd = nextEnd;
  }

  ranges.push({ start: rangeStart, end: rangeEnd });

  const hunks: CodeDiffHunk[] = [];
  const rows: CodeDiffRow[] = [];
  let insertedLineCount = 0;
  let deletedLineCount = 0;
  let unchangedLineCount = 0;

  for (let hunkIndex = 0; hunkIndex < ranges.length; hunkIndex += 1) {
    const range = ranges[hunkIndex];
    const hunkRows = operations.slice(range.start, range.end + 1).map((op) => opToRow(op, hunkIndex));

    for (const row of hunkRows) {
      if (row.kind === 'insert') {
        insertedLineCount += 1;
      } else if (row.kind === 'delete') {
        deletedLineCount += 1;
      } else {
        unchangedLineCount += 1;
      }
    }

    let oldStartLine: number | null = null;
    let oldEndLine: number | null = null;
    let newStartLine: number | null = null;
    let newEndLine: number | null = null;

    for (const row of hunkRows) {
      const bounds = rowLineBounds(row);
      const oldStart = bounds.oldStart;
      const oldEnd = bounds.oldEnd;
      const newStart = bounds.newStart;
      const newEnd = bounds.newEnd;

      if (oldStart !== null && oldEnd !== null) {
        oldStartLine = oldStartLine === null ? oldStart : Math.min(oldStartLine, oldStart);
        oldEndLine = oldEndLine === null ? oldEnd : Math.max(oldEndLine, oldEnd);
      }

      if (newStart !== null && newEnd !== null) {
        newStartLine = newStartLine === null ? newStart : Math.min(newStartLine, newStart);
        newEndLine = newEndLine === null ? newEnd : Math.max(newEndLine, newEnd);
      }
    }

    hunks.push({
      index: hunkIndex,
      oldStartLine,
      oldEndLine,
      newStartLine,
      newEndLine,
      rows: hunkRows,
    });
    rows.push(...hunkRows);
  }

  return {
    hunks,
    rows,
    summary: {
      oldLineCount: oldLines.length,
      newLineCount: newLines.length,
      unchangedLineCount,
      insertedLineCount,
      deletedLineCount,
      changedLineCount: insertedLineCount + deletedLineCount,
      hunkCount: hunks.length,
      isEmpty: insertedLineCount === 0 && deletedLineCount === 0,
    },
  };
}
