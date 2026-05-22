import {
  HighlightStyle,
  LanguageSupport,
  StreamLanguage,
  StringStream,
  syntaxHighlighting,
} from '@codemirror/language';
import { tags } from '@lezer/highlight';

/** Structural authoring forms. */
const ECKY_FORMS = new Set([
  'model',
  'part',
  'feature',
  'params',
  'build',
  'shape',
  'result',
  'verify',
  'tag',
  'metric',
  'expect',
  'define',
  'define-component',
  'define-syntax',
  'let',
  'let*',
  'lambda',
  'if',
  'cond',
  'when',
  'unless',
  'begin',
  'quote',
  'meta',
]);

/** Parameter/signature entry kinds. */
const ECKY_PARAM_KINDS = new Set(['number', 'toggle', 'select', 'image', 'option', 'text']);

/** Geometry operations (mirrors `ecky/cad` exports). */
const ECKY_CAD_OPS = new Set([
  'hole',
  'compound',
  'fuse',
  'cut',
  'common',
  'box',
  'sphere',
  'cylinder',
  'cone',
  'circle',
  'ring',
  'rectangle',
  'rounded-rect',
  'rounded-polygon',
  'polygon',
  'extrude',
  'revolve',
  'loft',
  'sweep',
  'helical-ridge',
  'shell',
  'offset',
  'offset-rounded',
  'fillet',
  'chamfer',
  'taper',
  'translate',
  'rotate',
  'scale',
  'mirror',
  'sampled-radial-loft',
  'bezier-path',
  'bspline',
  'path',
  'polyline',
  'profile',
  'make-face',
  'union',
  'difference',
  'intersection',
  'xor',
  'linear-array',
  'radial-array',
  'grid-array',
  'arc-array',
  'text',
  'svg',
  'import-stl',
  'path-frame',
  'plane',
  'location',
  'place',
  'clip-box',
  'twist',
  'repeat',
  'repeat-union',
  'repeat-compound',
  'repeat-pick',
  'for-union',
  'for-compound',
  'wall-pattern',
  'instance',
]);

/** Core helpers (mirrors `ecky/core` exports plus common scheme math). */
const ECKY_HELPERS = new Set([
  'vec2',
  'vec3',
  'start',
  'end',
  'xy',
  'yz',
  'xz',
  'zip',
  'enumerate',
  'flat-map',
  'concat-map',
  'linspace',
  'pi',
  'tau',
  'clamp',
  'lerp',
  'invlerp',
  'remap',
  'deg',
  'rad',
  'deg->rad',
  'rad->deg',
  'smoothstep',
  'square',
  'cube',
  'hash01',
  'hash-signed',
  'noise2',
  'fbm2',
  'voronoi2',
  'cell-distance2',
  'jitter2',
  'jittered-grid',
  'polar-points',
  'organic-loop',
  'wave-loop',
  'superellipse-point',
  'voronoi-cells',
  'lorenz-points',
  'rossler-points',
  'logistic-bifurcation-points',
  'henon-points',
  'map',
  'filter',
  'fold',
  'foldl',
  'foldr',
  'range',
  'append',
  'reverse',
  'list',
  'cons',
  'car',
  'cdr',
  'apply',
  'min',
  'max',
  'abs',
  'sqrt',
  'sin',
  'cos',
  'tan',
  'atan',
  'atan2',
  'floor',
  'ceiling',
  'round',
  'expt',
  'modulo',
  'not',
  'and',
  'or',
]);

/** Heads whose next symbol is a user-given name worth its own color. */
const ECKY_NAMING_FORMS = new Set(['part', 'feature', 'define-component', 'define', 'shape']);

type EckyHighlightState = {
  depth: number;
  afterOpen: boolean;
  expectName: boolean;
};

function isSymbolChar(ch: string): boolean {
  return /[A-Za-z0-9_?!+\-*/<>=.$]/.test(ch);
}

function classifySymbol(symbol: string, state: EckyHighlightState): string {
  const head = state.afterOpen;
  state.afterOpen = false;

  if (state.expectName) {
    state.expectName = false;
    return 'name';
  }
  if (ECKY_FORMS.has(symbol)) {
    if (head && ECKY_NAMING_FORMS.has(symbol)) state.expectName = true;
    return 'keyword';
  }
  if (ECKY_PARAM_KINDS.has(symbol)) {
    // `(number width 12 ...)`: the key after a kind reads like a name.
    if (head) state.expectName = true;
    return 'kind';
  }
  if (ECKY_CAD_OPS.has(symbol)) return 'op';
  if (ECKY_HELPERS.has(symbol)) return 'helper';
  // Unknown head position: a user component/helper instantiation.
  if (head) return 'call';
  return 'symbol';
}

export function readEckyToken(stream: StringStream, state: EckyHighlightState): string | null {
  if (stream.eatSpace()) return null;

  const next = stream.peek();
  if (!next) return null;

  if (next === ';') {
    stream.skipToEnd();
    return 'comment';
  }

  if (next === '(') {
    stream.next();
    state.depth += 1;
    state.afterOpen = true;
    state.expectName = false;
    return `paren${((state.depth - 1) % 3) + 1}`;
  }
  if (next === ')') {
    stream.next();
    const depth = state.depth;
    state.depth = Math.max(0, state.depth - 1);
    state.afterOpen = false;
    state.expectName = false;
    return `paren${((Math.max(1, depth) - 1) % 3) + 1}`;
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
    state.afterOpen = false;
    return 'string';
  }

  if (next === ':') {
    stream.next();
    stream.eatWhile(isSymbolChar);
    state.afterOpen = false;
    return 'atom';
  }

  if (next === '#') {
    stream.next();
    stream.eatWhile(/[A-Za-z:]/);
    state.afterOpen = false;
    return 'atom';
  }

  if (stream.match(/^[+-]?(?:\d+(?:\.\d+)?|\.\d+)/)) {
    state.afterOpen = false;
    return 'number';
  }

  if (isSymbolChar(next)) {
    stream.eatWhile(isSymbolChar);
    return classifySymbol(stream.current(), state);
  }

  stream.next();
  state.afterOpen = false;
  return null;
}

export const eckyLanguage = StreamLanguage.define<EckyHighlightState>({
  name: 'ecky',
  startState: () => ({ depth: 0, afterOpen: false, expectName: false }),
  copyState: (state) => ({ ...state }),
  token: readEckyToken,
  tokenTable: {
    keyword: tags.keyword,
    kind: tags.className,
    op: tags.typeName,
    helper: tags.macroName,
    name: tags.definition(tags.variableName),
    call: tags.function(tags.variableName),
    comment: tags.comment,
    string: tags.string,
    number: tags.number,
    atom: tags.atom,
    symbol: tags.variableName,
    paren1: tags.bracket,
    paren2: tags.squareBracket,
    paren3: tags.angleBracket,
  },
});

const eckyHighlightStyle = HighlightStyle.define([
  { tag: tags.keyword, class: 'cm-ecky-keyword' },
  { tag: tags.className, class: 'cm-ecky-kind' },
  { tag: tags.typeName, class: 'cm-ecky-op' },
  { tag: tags.macroName, class: 'cm-ecky-helper' },
  { tag: tags.definition(tags.variableName), class: 'cm-ecky-name' },
  { tag: tags.function(tags.variableName), class: 'cm-ecky-call' },
  { tag: tags.comment, class: 'cm-ecky-comment' },
  { tag: tags.string, class: 'cm-ecky-string' },
  { tag: tags.number, class: 'cm-ecky-number' },
  { tag: tags.atom, class: 'cm-ecky-atom' },
  { tag: tags.variableName, class: 'cm-ecky-symbol' },
  { tag: tags.bracket, class: 'cm-ecky-paren-1' },
  { tag: tags.squareBracket, class: 'cm-ecky-paren-2' },
  { tag: tags.angleBracket, class: 'cm-ecky-paren-3' },
]);

export function eckyLanguageSupport(): LanguageSupport {
  return new LanguageSupport(eckyLanguage, [syntaxHighlighting(eckyHighlightStyle)]);
}
