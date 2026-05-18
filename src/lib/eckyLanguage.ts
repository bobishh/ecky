import {
  HighlightStyle,
  LanguageSupport,
  StreamLanguage,
  StringStream,
  syntaxHighlighting,
} from '@codemirror/language';
import { tags } from '@lezer/highlight';

const ECKY_KEYWORDS = new Set([
  'model',
  'params',
  'number',
  'toggle',
  'select',
  'part',
  'feature',
  'build',
  'shape',
  'result',
  'verify',
  'tag',
  'metric',
  'expect',
  'define',
  'let',
  'if',
  'cond',
  'box',
  'cylinder',
  'sphere',
  'union',
  'difference',
  'intersection',
  'translate',
  'rotate',
  'scale',
  'extrude',
  'polygon',
  'repeat',
  'instance',
]);

function isSymbolChar(ch: string): boolean {
  return /[A-Za-z0-9_?!+\-*/<>=.$]/.test(ch);
}

export function readEckyToken(stream: StringStream): string | null {
  if (stream.eatSpace()) return null;

  const next = stream.peek();
  if (!next) return null;

  if (next === ';') {
    stream.skipToEnd();
    return 'comment';
  }

  if (next === '(' || next === ')') {
    stream.next();
    return 'paren';
  }

  if (next === '"') {
    stream.next();
    let escaped = false;
    while (!stream.eol()) {
      const ch = stream.next();
      if (!ch) break;
      if (escaped) {
        escaped = false;
        continue;
      }
      if (ch === '\\') {
        escaped = true;
        continue;
      }
      if (ch === '"') break;
    }
    return 'string';
  }

  if (next === ':') {
    stream.next();
    stream.eatWhile(isSymbolChar);
    return 'atom';
  }

  if (next === '#') {
    stream.next();
    stream.eatWhile(/[A-Za-z]/);
    return 'atom';
  }

  if (stream.match(/^[+-]?(?:\d+(?:\.\d+)?|\.\d+)/)) {
    return 'number';
  }

  if (isSymbolChar(next)) {
    stream.eatWhile(isSymbolChar);
    const symbol = stream.current();
    return ECKY_KEYWORDS.has(symbol) ? 'keyword' : 'symbol';
  }

  stream.next();
  return null;
}

export const eckyLanguage = StreamLanguage.define({
  name: 'ecky',
  token: readEckyToken,
  tokenTable: {
    keyword: tags.keyword,
    comment: tags.comment,
    string: tags.string,
    number: tags.number,
    atom: tags.atom,
    symbol: tags.variableName,
    paren: tags.bracket,
  },
});

export const eckyHighlightStyle = HighlightStyle.define([
  { tag: tags.keyword, class: 'cm-ecky-keyword' },
  { tag: tags.comment, class: 'cm-ecky-comment' },
  { tag: tags.string, class: 'cm-ecky-string' },
  { tag: tags.number, class: 'cm-ecky-number' },
  { tag: tags.atom, class: 'cm-ecky-atom' },
  { tag: tags.variableName, class: 'cm-ecky-symbol' },
  { tag: tags.bracket, class: 'cm-ecky-paren' },
]);

export function eckyLanguageSupport(): LanguageSupport {
  return new LanguageSupport(eckyLanguage, [syntaxHighlighting(eckyHighlightStyle)]);
}
