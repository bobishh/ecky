import assert from 'node:assert/strict';
import test from 'node:test';

import { resolveRelayPresence } from './relayPresence';
import { buildAgentGenieTraits } from '../genie/traits';
import type { AutoAgent } from '../types/domain';

const agents: AutoAgent[] = [
  { id: 'a1', label: 'Alpha', cmd: 'codex', args: [], enabled: true },
  { id: 'a2', label: 'Beta', cmd: 'gemini', args: [], enabled: true },
];

test('returns null when connection is not MCP', () => {
  assert.equal(
    resolveRelayPresence({
      source: 'threadAgentMascot',
      connectionType: 'api',
      autoAgents: agents,
      primaryAgentId: 'a1',
      senderLabel: 'Beta',
    }),
    null,
  );
});

test('returns null when bubble source is not agent/thread provenance', () => {
  assert.equal(
    resolveRelayPresence({
      source: 'assistant',
      connectionType: 'mcp',
      autoAgents: agents,
      primaryAgentId: 'a1',
      senderLabel: 'Beta',
    }),
    null,
  );
});

test('returns null when the sending agent is the primary agent', () => {
  assert.equal(
    resolveRelayPresence({
      source: 'threadAgentMascot',
      connectionType: 'mcp',
      autoAgents: agents,
      primaryAgentId: 'a1',
      senderLabel: 'Alpha',
    }),
    null,
  );
});

test('returns hue+label for an MCP non-primary agent-sourced bubble', () => {
  const result = resolveRelayPresence({
    source: 'threadAgentMascot',
    connectionType: 'mcp',
    autoAgents: agents,
    primaryAgentId: 'a1',
    senderLabel: 'Beta',
  });
  assert.ok(result);
  assert.equal(result.label, 'Beta');
  assert.equal(result.hue, buildAgentGenieTraits('Beta').colorHue);
});

test('relay hue is deterministic for a given identity', () => {
  const first = resolveRelayPresence({
    source: 'threadError',
    connectionType: 'mcp',
    autoAgents: agents,
    primaryAgentId: 'a1',
    senderLabel: 'Beta',
  });
  const second = resolveRelayPresence({
    source: 'threadAgentActivity',
    connectionType: 'mcp',
    autoAgents: agents,
    primaryAgentId: 'a1',
    senderLabel: 'Beta',
  });
  assert.ok(first && second);
  assert.equal(first.hue, second.hue);
});

test('returns null when no sender label is present', () => {
  assert.equal(
    resolveRelayPresence({
      source: 'threadAgentMascot',
      connectionType: 'mcp',
      autoAgents: agents,
      primaryAgentId: 'a1',
      senderLabel: null,
    }),
    null,
  );
});
