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
    warnings: [],
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
    primitiveId: 'primitive-front',
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
    primitiveId: 'primitive-front',
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

test('buildSketchTopologyRepairProposals skips bounds mismatch repairable by envelope snap', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    primitiveId: 'primitive-front',
    severity: 'error',
    message: 'Front bounds mismatch: sketch minX=10 maxX=60 minY=20 maxY=50, brep minX=0 maxX=80 minY=0 maxY=40.',
  };

  const proposals = buildSketchTopologyRepairProposals(document, projection(issue));

  assert.deepEqual(proposals, []);
});

test('buildSketchTopologyRepairProposals classifies concavity mismatch separately', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    primitiveId: 'primitive-front',
    severity: 'error',
    message: 'raw BREP/SKETCH concavity mismatch: sketch is convex but visible projection has concave turn',
  };

  const proposals = buildSketchTopologyRepairProposals(document, projection(issue));

  assert.equal(proposals[0]?.kind, 'concavity');
  assert.equal(proposals[0]?.view, 'front');
  assert.equal(proposals[0]?.issue, issue);
});

test('applySketchTopologyRepairProposal replaces target primitive with projection loop and preserves ids', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    primitiveId: 'primitive-front',
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
  });
  assert.notEqual(result.document, document);
});

test('applySketchTopologyRepairProposal returns error for missing projection view', () => {
  const issue: SketchValidationIssue = {
    sketchId: 'sketch-front',
    primitiveId: 'primitive-front',
    severity: 'error',
    message: 'raw BREP/SKETCH topology mismatch: face loop cannot be matched',
  };
  const response = { ...projection(issue), views: [] };
  const proposal = buildSketchTopologyRepairProposals(document, projection(issue))[0];

  assert.deepEqual(applySketchTopologyRepairProposal(document, response, proposal?.proposalId ?? ''), {
    error: 'Topology repair projection view missing.',
  });
});
