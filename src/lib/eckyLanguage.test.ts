import assert from 'node:assert/strict';
import test from 'node:test';

import { StringStream } from '@codemirror/language';

import { readEckyToken } from './eckyLanguage';

function tokenize(line: string, state = { depth: 0, afterOpen: false, expectName: false }) {
  const stream = new StringStream(line, 2, 2);
  const tokens: Array<{ text: string; token: string | null }> = [];
  while (!stream.eol()) {
    stream.start = stream.pos;
    const token = readEckyToken(stream, state);
    if (stream.pos === stream.start) {
      throw new Error(`Tokenizer stalled on ${JSON.stringify(line)} at ${stream.pos}`);
    }
    const text = stream.current();
    if (token) tokens.push({ text, token });
  }
  return tokens;
}

test('classifies ecky comment keyword number atom and string tokens', () => {
  const tokens = tokenize('; shell');
  assert.deepEqual(tokens, [{ text: '; shell', token: 'comment' }]);

  assert.deepEqual(tokenize('(model'), [
    { text: '(', token: 'paren1' },
    { text: 'model', token: 'keyword' },
  ]);

  assert.deepEqual(tokenize('(number width 10 :label "Width")'), [
    { text: '(', token: 'paren1' },
    { text: 'number', token: 'kind' },
    { text: 'width', token: 'name' },
    { text: '10', token: 'number' },
    { text: ':label', token: 'atom' },
    { text: '"Width"', token: 'string' },
    { text: ')', token: 'paren1' },
  ]);
});

test('separates geometry ops, helpers, and user calls from plain symbols', () => {
  assert.deepEqual(tokenize('(difference (cylinder (* 2 pin_d) 10 96) (knuckle :pin_d 6))'), [
    { text: '(', token: 'paren1' },
    { text: 'difference', token: 'op' },
    { text: '(', token: 'paren2' },
    { text: 'cylinder', token: 'op' },
    { text: '(', token: 'paren3' },
    { text: '*', token: 'call' },
    { text: '2', token: 'number' },
    { text: 'pin_d', token: 'symbol' },
    { text: ')', token: 'paren3' },
    { text: '10', token: 'number' },
    { text: '96', token: 'number' },
    { text: ')', token: 'paren2' },
    { text: '(', token: 'paren2' },
    { text: 'knuckle', token: 'call' },
    { text: ':pin_d', token: 'atom' },
    { text: '6', token: 'number' },
    { text: ')', token: 'paren2' },
    { text: ')', token: 'paren1' },
  ]);

  assert.deepEqual(tokenize('(map vec2 (linspace 0 1 8))').map((entry) => entry.token), [
    'paren1',
    'helper',
    'helper',
    'paren2',
    'helper',
    'number',
    'number',
    'number',
    'paren2',
    'paren1',
  ]);
});

test('part and component names get their own token', () => {
  assert.deepEqual(tokenize('(part hinge_a (knuckle :pin_d 6))').map((entry) => entry.token), [
    'paren1',
    'keyword',
    'name',
    'paren2',
    'call',
    'atom',
    'number',
    'paren2',
    'paren1',
  ]);

  assert.deepEqual(tokenize('(define-component knuckle ((number pin_d 8)) (box 1 1 1))').map((entry) => entry.token), [
    'paren1',
    'keyword',
    'name',
    'paren2',
    'paren3',
    'kind',
    'name',
    'number',
    'paren3',
    'paren2',
    'paren2',
    'op',
    'number',
    'number',
    'number',
    'paren2',
    'paren1',
  ]);
});

test('paren depth cycles through three rainbow classes and state survives lines', () => {
  const state = { depth: 0, afterOpen: false, expectName: false };
  tokenize('(model', state);
  const tokens = tokenize('  (part body (box 1 1 1))', state);
  assert.equal(tokens[0]?.token, 'paren2');
  assert.equal(tokens[3]?.token, 'paren3');
});
