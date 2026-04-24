import assert from 'node:assert/strict';
import test from 'node:test';

import {
  nextPrimitiveSequenceFromStrokes,
  parseSketchDocumentImportSource,
  parseSketchDocumentJson,
  parseSketchDocumentSource,
  sketchDocumentJsonToStrokes,
  sketchDocumentToStrokes,
} from './sketchDocumentReplay';
import type { SketchDocument, SketchSuggestionRequest } from './tauri/contracts';
import type { SketchStroke } from './sketchWorkspaceState';

const document: SketchDocument = {
  documentId: 'doc-replay',
  sketches: [
    {
      sketchId: 'sketch-front',
      view: 'front',
      primitives: [
        {
          primitiveId: 'primitive-front-7',
          kind: 'polyline',
          points: [
            [10, 20],
            [40, 20],
            [40, 50],
            [10, 50],
            [10, 20],
          ],
          closed: true,
        },
      ],
    },
    {
      sketchId: 'sketch-top',
      view: 'top',
      primitives: [
        {
          primitiveId: 'primitive-top-3',
          kind: 'polyline',
          points: [
            [5, 5],
            [25, 5],
            [15, 30],
            [5, 5],
          ],
          closed: true,
        },
      ],
    },
  ],
  activeSketchId: 'sketch-front',
  units: 'mm',
};

const strokes: SketchStroke[] = [
  {
    primitiveId: 'primitive-front-7',
    view: 'front',
    points: [
      [10, 20],
      [40, 20],
      [40, 50],
      [10, 50],
      [10, 20],
    ],
    closed: true,
  },
  {
    primitiveId: 'primitive-top-12',
    view: 'top',
    points: [
      [5, 5],
      [25, 5],
      [15, 30],
      [5, 5],
    ],
    closed: true,
  },
];

test('parseSketchDocumentSource unwraps SketchSuggestionRequest document', () => {
  const request: SketchSuggestionRequest = {
    document,
  };

  const result = parseSketchDocumentSource(request);

  assert.ok(!('error' in result));
  assert.equal(result.document.documentId, 'doc-replay');
});

test('parseSketchDocumentJson unwraps SketchSuggestionRequest document', () => {
  const result = parseSketchDocumentJson(JSON.stringify({ document }));

  assert.ok(!('error' in result));
  assert.equal(result.document.documentId, 'doc-replay');
});

test('parseSketchDocumentImportSource accepts raw ecky source with embedded sketch document envelope', () => {
  const encoded = Buffer.from(JSON.stringify(document), 'utf8').toString('base64');
  const result = parseSketchDocumentImportSource(`; ecky-sketch-document-base64: ${encoded}\n(model (part body))`);

  assert.ok(!('error' in result));
  assert.equal(result.document.documentId, 'doc-replay');
});

test('parseSketchDocumentImportSource keeps JSON parser errors for JSON-looking input', () => {
  const result = parseSketchDocumentImportSource('{"documentId":"broken"');

  assert.deepEqual(result, {
    error: 'Sketch document JSON is invalid: Unexpected end of JSON input',
  });
});

test('parseSketchDocumentImportSource returns envelope marker errors for non-JSON source', () => {
  const result = parseSketchDocumentImportSource('(model (part body (box 1 1 1)))');

  assert.deepEqual(result, {
    error: 'Sketch document marker missing.',
  });
});

test('sketchDocumentToStrokes replays closed polylines with primitiveId and view', () => {
  const result = sketchDocumentToStrokes(document);

  assert.ok(!('error' in result));
  assert.deepEqual(result.strokes, [
    {
      primitiveId: 'primitive-front-7',
      view: 'front',
      points: [
        [10, 20],
        [40, 20],
        [40, 50],
        [10, 50],
        [10, 20],
      ],
      closed: true,
    },
    {
      primitiveId: 'primitive-top-3',
      view: 'top',
      points: [
        [5, 5],
        [25, 5],
        [15, 30],
        [5, 5],
      ],
      closed: true,
    },
  ]);
});

test('sketchDocumentJsonToStrokes replays closed polylines from JSON text', () => {
  const result = sketchDocumentJsonToStrokes(JSON.stringify({ document }));

  assert.ok(!('error' in result));
  assert.deepEqual(result.strokes, [
    {
      primitiveId: 'primitive-front-7',
      view: 'front',
      points: [
        [10, 20],
        [40, 20],
        [40, 50],
        [10, 50],
        [10, 20],
      ],
      closed: true,
    },
    {
      primitiveId: 'primitive-top-3',
      view: 'top',
      points: [
        [5, 5],
        [25, 5],
        [15, 30],
        [5, 5],
      ],
      closed: true,
    },
  ]);
});

test('sketchDocumentToStrokes replays dimension locks from dimension constraints', () => {
  const result = sketchDocumentToStrokes({
    ...document,
    sketches: [
      {
        sketchId: 'sketch-front',
        view: 'front',
        primitives: [
          {
            primitiveId: 'primitive-front-7',
            kind: 'polyline',
            points: [
              [10, 20],
              [40, 20],
              [40, 50],
              [10, 50],
              [10, 20],
            ],
            closed: true,
          },
        ],
        constraints: [
          { constraintId: 'primitive-front-7-closed', kind: 'closed', targetIds: ['primitive-front-7'] },
          {
            constraintId: 'primitive-front-7-width-dimension',
            kind: 'dimension',
            targetIds: ['primitive-front-7'],
            value: 30,
          },
          {
            constraintId: 'primitive-front-7-height-dimension',
            kind: 'dimension',
            targetIds: ['primitive-front-7'],
            value: 30,
          },
        ],
      },
    ],
  });

  assert.ok(!('error' in result));
  assert.deepEqual(result.strokes[0]?.dimensionLocks, { width: true, height: true });
});

test('sketchDocumentToStrokes rejects dimension constraints that do not match primitive bounds', () => {
  assert.deepEqual(
    sketchDocumentToStrokes({
      documentId: 'doc-dimension-mismatch',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-46',
              kind: 'polyline',
              points: [
                [12, 18],
                [58, 18],
                [58, 49],
                [12, 49],
                [12, 18],
              ],
              closed: true,
            },
          ],
          constraints: [
            { constraintId: 'primitive-front-46-closed', kind: 'closed', targetIds: ['primitive-front-46'] },
            {
              constraintId: 'primitive-front-46-width-dimension',
              kind: 'dimension',
              targetIds: ['primitive-front-46'],
              value: 99,
            },
          ],
        },
      ],
      activeSketchId: 'sketch-front',
      units: 'mm',
    }),
    {
      error: "sketch 'sketch-front' primitive 'primitive-front-46' width dimension expected 99mm but measured 46mm.",
    },
  );
});

test('nextPrimitiveSequenceFromStrokes returns highest primitive suffix', () => {
  assert.equal(nextPrimitiveSequenceFromStrokes(strokes), 12);
});

test('sketchDocumentToStrokes rejects missing document', () => {
  assert.deepEqual(sketchDocumentToStrokes(null), {
    error: 'Sketch document unavailable.',
  });
});

test('parseSketchDocumentJson rejects empty input', () => {
  assert.deepEqual(parseSketchDocumentJson('   '), {
    error: 'Sketch document JSON is empty.',
  });
});

test('parseSketchDocumentJson rejects invalid JSON with parser message', () => {
  const result = parseSketchDocumentJson('{');

  assert.ok('error' in result);
  assert.equal(result.error, 'Sketch document JSON is invalid: Unexpected end of JSON input');
});

test('parseSketchDocumentJson rejects missing document or sketches', () => {
  assert.deepEqual(parseSketchDocumentJson('{}'), {
    error: 'Sketch document JSON missing document/sketches.',
  });
});

test('sketchDocumentToStrokes rejects empty sketches', () => {
  assert.deepEqual(
    sketchDocumentToStrokes({
      documentId: 'doc-empty',
      sketches: [],
      activeSketchId: null,
      units: 'mm',
    }),
    {
      error: 'Sketch document has no sketches.',
    },
  );
});

test('sketchDocumentJsonToStrokes rejects unsupported primitive kind', () => {
  assert.deepEqual(
    sketchDocumentJsonToStrokes(
      JSON.stringify({
        document: {
          documentId: 'doc-kind',
          sketches: [
            {
              sketchId: 'sketch-front',
              view: 'front',
              primitives: [
                {
                  primitiveId: 'primitive-front-1',
                  kind: 'circle',
                  points: [[10, 20]],
                  closed: true,
                },
              ],
            },
          ],
          activeSketchId: 'sketch-front',
          units: 'mm',
        },
      }),
    ),
    {
      error: "sketch 'sketch-front' primitive 'primitive-front-1' has unsupported kind 'circle'.",
    },
  );
});

test('sketchDocumentJsonToStrokes rejects invalid points', () => {
  assert.deepEqual(
    sketchDocumentJsonToStrokes(
      JSON.stringify({
        document: {
          documentId: 'doc-points',
          sketches: [
            {
              sketchId: 'sketch-front',
              view: 'front',
              primitives: [
                {
                  primitiveId: 'primitive-front-1',
                  kind: 'polyline',
                  points: [
                    [10, 20],
                    [Number.NaN, 30],
                  ],
                  closed: true,
                },
              ],
            },
          ],
          activeSketchId: 'sketch-front',
          units: 'mm',
        },
      }),
    ),
    {
      error: "sketch 'sketch-front' primitive 'primitive-front-1' has invalid point at index 1.",
    },
  );
});

test('sketchDocumentJsonToStrokes rejects open primitive', () => {
  assert.deepEqual(
    sketchDocumentJsonToStrokes(
      JSON.stringify({
        document: {
          documentId: 'doc-open',
          sketches: [
            {
              sketchId: 'sketch-front',
              view: 'front',
              primitives: [
                {
                  primitiveId: 'primitive-front-1',
                  kind: 'polyline',
                  points: [
                    [10, 20],
                    [40, 20],
                    [40, 50],
                    [10, 50],
                  ],
                  closed: false,
                },
              ],
            },
          ],
          activeSketchId: 'sketch-front',
          units: 'mm',
        },
      }),
    ),
    {
      error: "sketch 'sketch-front' primitive 'primitive-front-1' is not closed.",
    },
  );
});

test('sketchDocumentToStrokes rejects unsupported primitive kind', () => {
  assert.deepEqual(
    sketchDocumentToStrokes({
      documentId: 'doc-kind',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-1',
              kind: 'circle',
              points: [[10, 20]],
              closed: true,
            },
          ],
        },
      ],
      activeSketchId: 'sketch-front',
      units: 'mm',
    }),
    {
      error: "sketch 'sketch-front' primitive 'primitive-front-1' has unsupported kind 'circle'.",
    },
  );
});

test('sketchDocumentToStrokes rejects invalid points', () => {
  assert.deepEqual(
    sketchDocumentToStrokes({
      documentId: 'doc-points',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-1',
              kind: 'polyline',
              points: [
                [10, 20],
                [Number.NaN, 30],
              ],
              closed: true,
            },
          ],
        },
      ],
      activeSketchId: 'sketch-front',
      units: 'mm',
    }),
    {
      error: "sketch 'sketch-front' primitive 'primitive-front-1' has invalid point at index 1.",
    },
  );
});

test('sketchDocumentToStrokes rejects open primitive', () => {
  assert.deepEqual(
    sketchDocumentToStrokes({
      documentId: 'doc-open',
      sketches: [
        {
          sketchId: 'sketch-front',
          view: 'front',
          primitives: [
            {
              primitiveId: 'primitive-front-1',
              kind: 'polyline',
              points: [
                [10, 20],
                [40, 20],
                [40, 50],
                [10, 50],
              ],
              closed: false,
            },
          ],
        },
      ],
      activeSketchId: 'sketch-front',
      units: 'mm',
    }),
    {
      error: "sketch 'sketch-front' primitive 'primitive-front-1' is not closed.",
    },
  );
});
