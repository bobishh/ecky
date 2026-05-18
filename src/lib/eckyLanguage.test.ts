import assert from 'node:assert/strict';
import test from 'node:test';

import { StringStream } from '@codemirror/language';

import { readEckyToken } from './eckyLanguage';

function tokenize(line: string) {
  const stream = new StringStream(line, 2, 2);
  const tokens: Array<{ text: string; token: string | null }> = [];
  while (!stream.eol()) {
    stream.start = stream.pos;
    const token = readEckyToken(stream);
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
    { text: '(', token: 'paren' },
    { text: 'model', token: 'keyword' },
  ]);

  assert.deepEqual(tokenize('(number width 10 :label "Width")'), [
    { text: '(', token: 'paren' },
    { text: 'number', token: 'keyword' },
    { text: 'width', token: 'symbol' },
    { text: '10', token: 'number' },
    { text: ':label', token: 'atom' },
    { text: '"Width"', token: 'string' },
    { text: ')', token: 'paren' },
  ]);
});
