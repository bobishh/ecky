import assert from 'node:assert/strict';
import test from 'node:test';

import {
  applySketchTopologyRepairProposal,
  buildSketchTopologyRepairProposals,
  type SketchTopologyRepairResult,
} from './sketchTopologyRepairProposal';
import type { BrepHiddenLineProjectionResponse, SketchDocument, SketchValidationIssue } from './tauri/contracts';

const document: SketchDocument = {
  documentId: 'doc-topology-proposal',
  activeSketchId: 'sketch-front',
  units: 'mm',
  sketches: [
    {
      sketchId: 'sketch-front',
      view: 'front',
      primitives: [
        {
          primitiveId: 'primitive-front',
          kind: 'polyline',
          points: [
            [10, 20],
            [60, 20],
            [60, 50],
            [10, 50],
            [10, 20],
          ],
          closed: true,
        },
      ],
    },
  ],
};

function projection(issue: SketchValidationIssue): BrepHiddenLineProjectionResponse {
  return {
    modelId: 'model-1',
    sourceArtifactPath: '/tmp/model.step',
    views: [
      {
        view: 'front',
        direction: [0, -1, 0],
        visibleEdges: [
          { edgeId: 'front-bottom', points: [[10, 20], [60, 20]], sourceClass: 'V' },
          { edgeId: 'front-right', points: [[60, 20], [60, 50]], sourceClass: 'V' },
          { edgeId: 'front-top', points: [[60, 50], [10, 50]], sourceClass: 'V' },
          { edgeId: 'front-left', points: [[10, 50], [10, 20]], sourceClass: 'V' },
        ],
        hiddenEdges: [],
      },
    ],
    validation: {
      passed: false,
      issues: [issue],
      evidence: [],
    },
  };
}

function assertRepairSuccess(result: SketchTopologyRepairResult): Extract<SketchTopologyRepairResult, { document: SketchDocument }> {
  if ('error' in result) {
    assert.fail(result.error);
  }
  return result;
}

test('buildSketchTopologyRepairProposals flags same-bounds containment mismatch for manual redraw', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    view: 'front',
    primitiveId: 'primitive-front',
    kind: 'containmentMismatch',
    severity: 'error',
    message: 'raw BREP/SKETCH containment mismatch: front visible edge samples outside source profile while bounds match exactly',
  };

  const proposals = buildSketchTopologyRepairProposals(document, projection(issue));

  assert.deepEqual(proposals, [
    {
      proposalId: 'topology-repair-sketch-front-primitive-front-0',
      kind: 'manual-redraw',
      sketchId: 'sketch-front',
      primitiveId: 'primitive-front',
      view: 'front',
      issue,
      reason: 'Containment mismatch with matching projection bounds needs topology redraw.',
    },
  ]);
  assert.deepEqual(document.sketches?.[0]?.primitives?.[0]?.points?.[0], [10, 20]);
});

test('buildSketchTopologyRepairProposals turns unknown topology mismatch into typed proposal', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    view: 'front',
    primitiveId: 'primitive-front',
    kind: 'topologyMismatch',
    severity: 'error',
    message: 'raw BREP/SKETCH topology mismatch: face loop cannot be matched',
  };

  const proposals = buildSketchTopologyRepairProposals(document, projection(issue));

  assert.equal(proposals.length, 1);
  assert.equal(proposals[0]?.kind, 'topology');
  assert.equal(proposals[0]?.primitiveId, 'primitive-front');
  assert.equal(proposals[0]?.sketchId, 'sketch-front');
  assert.equal(proposals[0]?.view, 'front');
  assert.equal(proposals[0]?.issue, issue);
});

test('buildSketchTopologyRepairProposals skips stale unlocatable topology issue', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'ghost-sketch',
    view: 'top',
    primitiveId: 'ghost-primitive',
    kind: 'topologyMismatch',
    severity: 'error',
    message: 'closed loop cannot be matched',
  };

  const proposals = buildSketchTopologyRepairProposals(document, projection(issue));

  assert.deepEqual(proposals, []);
});

test('buildSketchTopologyRepairProposals skips bounds mismatch repairable by envelope snap', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    view: 'front',
    primitiveId: 'primitive-front',
    kind: 'boundsMismatch',
    severity: 'error',
    message: 'Front bounds mismatch: sketch minX=10 maxX=60 minY=20 maxY=50, brep minX=0 maxX=80 minY=0 maxY=40.',
  };

  const proposals = buildSketchTopologyRepairProposals(document, projection(issue));

  assert.deepEqual(proposals, []);
});

test('buildSketchTopologyRepairProposals classifies concavity mismatch separately', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    view: 'front',
    primitiveId: 'primitive-front',
    kind: 'concavityMismatch',
    severity: 'error',
    message: 'projection silhouette deviates from source profile',
  };

  const proposals = buildSketchTopologyRepairProposals(document, projection(issue));

  assert.equal(proposals[0]?.kind, 'concavity');
  assert.equal(proposals[0]?.view, 'front');
  assert.equal(proposals[0]?.issue, issue);
});

test('buildSketchTopologyRepairProposals uses structured topology kind when message is neutral', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    view: 'front',
    primitiveId: 'primitive-front',
    kind: 'topologyMismatch',
    severity: 'error',
    message: 'loop cannot be matched',
  };

  const proposals = buildSketchTopologyRepairProposals(document, projection(issue));

  assert.equal(proposals[0]?.kind, 'topology');
  assert.equal(proposals[0]?.view, 'front');
});

test('applySketchTopologyRepairProposal replaces target primitive with projection loop and preserves ids', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    view: 'front',
    primitiveId: 'primitive-front',
    kind: 'topologyMismatch',
    severity: 'error',
    message: 'raw BREP/SKETCH topology mismatch: face loop cannot be matched',
  };
  const response = projection(issue);
  const proposal = buildSketchTopologyRepairProposals(document, response)[0];

  const result = assertRepairSuccess(applySketchTopologyRepairProposal(document, response, proposal?.proposalId ?? ''));

  assert.equal(result.evidence.primitiveId, 'primitive-front');
  assert.match(result.evidence.detail, /TOPOLOGY REDRAW FRONT primitive-front/i);
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[0], {
    primitiveId: 'primitive-front',
    kind: 'polyline',
    points: [
      [10, 20],
      [60, 20],
      [60, 50],
      [10, 50],
      [10, 20],
    ],
    closed: true,
    topology: {
      loopId: 'derived-front-bottom-front-left-front-right-front-top',
      edgeIds: ['front-bottom', 'front-right', 'front-top', 'front-left'],
      loopRole: 'outer',
      sourceClass: 'V',
    },
  });
  assert.notEqual(result.document, document);
});

test('applySketchTopologyRepairProposal returns error for missing projection view', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    view: 'front',
    primitiveId: 'primitive-front',
    kind: 'topologyMismatch',
    severity: 'error',
    message: 'raw BREP/SKETCH topology mismatch: face loop cannot be matched',
  };
  const response = { ...projection(issue), views: [] };
  const proposal = buildSketchTopologyRepairProposals(document, projection(issue))[0];

  assert.deepEqual(applySketchTopologyRepairProposal(document, response, proposal?.proposalId ?? ''), {
    error: 'Topology repair projection view missing.',
  });
});

test('applySketchTopologyRepairProposal uses loop topology metadata before loop index for multi-loop repair target', () => {
  const multiLoopDocument: SketchDocument = {
    ...document,
    sketches: [
      {
        sketchId: 'sketch-front',
        view: 'front',
        primitives: [
          {
            primitiveId: 'primitive-front-hole',
            kind: 'polyline',
            points: [
              [30, 20],
              [40, 20],
              [40, 30],
              [30, 30],
              [30, 20],
            ],
            closed: true,
            topology: {
              loopId: 'front-hole',
              edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
              loopRole: 'hole',
              sourceClass: 'derived',
            },
          },
          {
            primitiveId: 'primitive-front-outer',
            kind: 'polyline',
            points: [
              [0, 0],
              [80, 0],
              [80, 50],
              [0, 50],
              [0, 0],
            ],
            closed: true,
            topology: {
              loopId: 'front-outer',
              edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
              loopRole: 'outer',
              sourceClass: 'derived',
            },
          },
        ],
      },
    ],
  };
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    view: 'front',
    primitiveId: 'primitive-front-hole',
    kind: 'topologyMismatch',
    severity: 'error',
    message: 'raw BREP/SKETCH topology mismatch: front face loop cannot be matched',
  };
  const response: BrepHiddenLineProjectionResponse = {
    modelId: 'model-1',
    sourceArtifactPath: '/tmp/model.step',
    views: [
      {
        view: 'front',
        direction: [0, -1, 0],
        visibleEdges: [],
        hiddenEdges: [],
        loops: [
          {
            loopId: 'front-outer',
            edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
            points: [
              [0, 0],
              [80, 0],
              [80, 50],
              [0, 50],
              [0, 0],
            ],
            role: 'outer',
            sourceClass: 'V',
          },
          {
            loopId: 'front-hole',
            edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
            points: [
              [25, 18],
              [45, 18],
              [45, 34],
              [25, 34],
              [25, 18],
            ],
            role: 'hole',
            sourceClass: 'V',
          },
        ],
      },
    ],
    validation: {
      passed: false,
      issues: [issue],
      evidence: [],
    },
  };
  const proposal = buildSketchTopologyRepairProposals(multiLoopDocument, response)[0];
  const result = assertRepairSuccess(
    applySketchTopologyRepairProposal(multiLoopDocument, response, proposal?.proposalId ?? ''),
  );

  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[0]?.points, [
    [25, 18],
    [45, 18],
    [45, 34],
    [25, 34],
    [25, 18],
  ]);
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[0]?.topology, {
    loopId: 'front-hole',
    edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
    loopRole: 'hole',
    sourceClass: 'V',
  });
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[1]?.points, [
    [0, 0],
    [80, 0],
    [80, 50],
    [0, 50],
    [0, 0],
  ]);
});

test('applySketchTopologyRepairProposal matches topology-only issue against derived edge-only loops', () => {
  const multiLoopDocument: SketchDocument = {
    ...document,
    sketches: [
      {
        sketchId: 'sketch-front',
        view: 'front',
        primitives: [
          {
            primitiveId: 'primitive-front-hole',
            kind: 'polyline',
            points: [
              [25, 18],
              [45, 18],
              [45, 34],
              [25, 34],
              [25, 18],
            ],
            closed: true,
            topology: {
              loopId: 'front-hole',
              edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
              loopRole: 'hole',
              sourceClass: 'derived',
            },
          },
          {
            primitiveId: 'primitive-front-outer',
            kind: 'polyline',
            points: [
              [0, 0],
              [80, 0],
              [80, 50],
              [0, 50],
              [0, 0],
            ],
            closed: true,
            topology: {
              loopId: 'front-outer',
              edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
              loopRole: 'outer',
              sourceClass: 'derived',
            },
          },
        ],
      },
    ],
  };
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    view: 'front',
    kind: 'topologyMismatch',
    severity: 'error',
    message: 'raw BREP/SKETCH topology mismatch: front face loop cannot be matched',
    topology: {
      edgeIds: ['inner-a', 'inner-b', 'inner-c', 'inner-d'],
      loopRole: 'hole',
    },
  };
  const response: BrepHiddenLineProjectionResponse = {
    modelId: 'model-1',
    sourceArtifactPath: '/tmp/model.step',
    views: [
      {
        view: 'front',
        direction: [0, -1, 0],
        visibleEdges: [
          { edgeId: 'outer-a', points: [[0, 0], [80, 0]], sourceClass: 'V' },
          { edgeId: 'outer-b', points: [[80, 0], [80, 50]], sourceClass: 'V' },
          { edgeId: 'outer-c', points: [[80, 50], [0, 50]], sourceClass: 'V' },
          { edgeId: 'outer-d', points: [[0, 50], [0, 0]], sourceClass: 'V' },
          { edgeId: 'inner-a', points: [[24, 17], [46, 17]], sourceClass: 'V' },
          { edgeId: 'inner-b', points: [[46, 17], [46, 35]], sourceClass: 'V' },
          { edgeId: 'inner-c', points: [[46, 35], [24, 35]], sourceClass: 'V' },
          { edgeId: 'inner-d', points: [[24, 35], [24, 17]], sourceClass: 'V' },
        ],
        hiddenEdges: [],
      },
    ],
    validation: {
      passed: false,
      issues: [issue],
      evidence: [],
    },
  };
  const proposal = buildSketchTopologyRepairProposals(multiLoopDocument, response)[0];
  const result = assertRepairSuccess(
    applySketchTopologyRepairProposal(multiLoopDocument, response, proposal?.proposalId ?? ''),
  );

  assert.equal(proposal?.primitiveId, 'primitive-front-hole');
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[0]?.points, [
    [24, 17],
    [46, 17],
    [46, 35],
    [24, 35],
    [24, 17],
  ]);
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[1]?.points, [
    [0, 0],
    [80, 0],
    [80, 50],
    [0, 50],
    [0, 0],
  ]);
});

test('applySketchTopologyRepairProposal keeps exact target hole under edge-id churn across sibling holes', () => {
  const multiLoopDocument: SketchDocument = {
    ...document,
    sketches: [
      {
        sketchId: 'sketch-front',
        view: 'front',
        primitives: [
          {
            primitiveId: 'primitive-front-hole-b',
            kind: 'polyline',
            points: [
              [50, 22],
              [64, 22],
              [64, 36],
              [50, 36],
              [50, 22],
            ],
            closed: true,
            topology: {
              loopId: 'front-hole-b',
              edgeIds: ['hole-b-a', 'hole-b-b', 'hole-b-c', 'hole-b-d'],
              loopRole: 'hole',
              sourceClass: 'derived',
            },
          },
          {
            primitiveId: 'primitive-front-hole-a',
            kind: 'polyline',
            points: [
              [12, 12],
              [22, 12],
              [22, 22],
              [12, 22],
              [12, 12],
            ],
            closed: true,
            topology: {
              loopId: 'front-hole-a',
              edgeIds: ['hole-a-a', 'hole-a-b', 'hole-a-c', 'hole-a-d'],
              loopRole: 'hole',
              sourceClass: 'derived',
            },
          },
          {
            primitiveId: 'primitive-front-outer',
            kind: 'polyline',
            points: [
              [0, 0],
              [80, 0],
              [80, 50],
              [0, 50],
              [0, 0],
            ],
            closed: true,
            topology: {
              loopId: 'front-outer',
              edgeIds: ['outer-a', 'outer-b', 'outer-c', 'outer-d'],
              loopRole: 'outer',
              sourceClass: 'derived',
            },
          },
        ],
      },
    ],
  };
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    view: 'front',
    primitiveId: 'primitive-front-hole-b',
    kind: 'topologyMismatch',
    severity: 'error',
    message: 'closed loop cannot be matched',
    topology: {
      loopId: 'front-hole-b',
      edgeIds: ['hole-b-a', 'hole-b-b', 'hole-b-c', 'hole-b-d'],
      loopRole: 'hole',
      sourceClass: 'derived',
    },
  };
  const response: BrepHiddenLineProjectionResponse = {
    modelId: 'model-1',
    sourceArtifactPath: '/tmp/model.step',
    views: [
      {
        view: 'front',
        direction: [0, -1, 0],
        visibleEdges: [
          { edgeId: 'outer-z0', points: [[0, 0], [80, 0]], sourceClass: 'V' },
          { edgeId: 'outer-z1', points: [[80, 0], [80, 50]], sourceClass: 'V' },
          { edgeId: 'outer-z2', points: [[80, 50], [0, 50]], sourceClass: 'V' },
          { edgeId: 'outer-z3', points: [[0, 50], [0, 0]], sourceClass: 'V' },
          { edgeId: 'hole-a-z0', points: [[12, 12], [22, 12]], sourceClass: 'V' },
          { edgeId: 'hole-a-z1', points: [[22, 12], [22, 22]], sourceClass: 'V' },
          { edgeId: 'hole-a-z2', points: [[22, 22], [12, 22]], sourceClass: 'V' },
          { edgeId: 'hole-a-z3', points: [[12, 22], [12, 12]], sourceClass: 'V' },
          { edgeId: 'hole-b-z0', points: [[52, 24], [66, 24]], sourceClass: 'V' },
          { edgeId: 'hole-b-z1', points: [[66, 24], [66, 38]], sourceClass: 'V' },
          { edgeId: 'hole-b-z2', points: [[66, 38], [52, 38]], sourceClass: 'V' },
          { edgeId: 'hole-b-z3', points: [[52, 38], [52, 24]], sourceClass: 'V' },
        ],
        hiddenEdges: [],
      },
    ],
    validation: {
      passed: false,
      issues: [issue],
      evidence: [],
    },
  };

  const proposal = buildSketchTopologyRepairProposals(multiLoopDocument, response)[0];
  const result = assertRepairSuccess(
    applySketchTopologyRepairProposal(multiLoopDocument, response, proposal?.proposalId ?? ''),
  );

  assert.equal(proposal?.primitiveId, 'primitive-front-hole-b');
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[0]?.points, [
    [52, 24],
    [66, 24],
    [66, 38],
    [52, 38],
    [52, 24],
  ]);
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[1]?.points, [
    [12, 12],
    [22, 12],
    [22, 22],
    [12, 22],
    [12, 12],
  ]);
  assert.deepEqual(result.document.sketches?.[0]?.primitives?.[2]?.points, [
    [0, 0],
    [80, 0],
    [80, 50],
    [0, 50],
    [0, 0],
  ]);
});
