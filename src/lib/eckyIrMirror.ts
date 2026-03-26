const CANON_TO_MIRROR: Record<string, string> = {
  model: 'scene',
  params: 'controls',
  part: 'piece',
  number: 'num',
  select: 'choice',
  toggle: 'flag',
  image: 'image',
  offset: 'inflate',
  'offset-rounded': 'inflate-round',
  union: 'merge',
  difference: 'cut',
  intersection: 'intersect',
  xor: 'exclusive',
  shell: 'hollow',
  'wall-pattern': 'surface',
  translate: 'move',
  mirror: 'flip',
  loft: 'blend',
  'grid-array': 'grid',
  'arc-array': 'arc',
  smoothstep: 'softstep',
  'linear-array': 'line-array',
  'radial-array': 'ring-array',
  profile: 'outline',
  'rounded-polygon': 'shape-round',
  bspline: 'curve',
  if: 'when',
  ':label': ':name',
  ':min': ':low',
  ':max': ':high',
  ':options': ':choices',
  ':frozen': ':locked',
  ':mode': ':style',
  ':depth': ':amount',
  ':uFreq': ':u',
  ':vFreq': ':v',
  ':softness': ':soft',
  ':twistDeg': ':twist',
  ':rimFade': ':fade',
  ':outer': ':rim',
  ':holes': ':cuts',
};

const MIRROR_TO_CANON = Object.fromEntries(
  Object.entries(CANON_TO_MIRROR).map(([canon, mirror]) => [mirror, canon]),
) as Record<string, string>;

function transformTokens(source: string, dictionary: Record<string, string>): string {
  let result = '';
  let token = '';
  let inString = false;
  let escaping = false;

  const flushToken = () => {
    if (!token) return;
    result += dictionary[token] ?? token;
    token = '';
  };

  const isTokenChar = (ch: string) => /[A-Za-z0-9:_-]/.test(ch);

  for (const ch of source) {
    if (inString) {
      result += ch;
      if (escaping) {
        escaping = false;
      } else if (ch === '\\') {
        escaping = true;
      } else if (ch === '"') {
        inString = false;
      }
      continue;
    }

    if (ch === '"') {
      flushToken();
      inString = true;
      result += ch;
      continue;
    }

    if (isTokenChar(ch)) {
      token += ch;
    } else {
      flushToken();
      result += ch;
    }
  }

  flushToken();
  return result;
}

export function isEckyIrSource(source: string | null | undefined): boolean {
  const text = `${source ?? ''}`.trimStart();
  return text.startsWith('(model') || text.startsWith('(scene');
}

export function toMirrorIr(source: string): string {
  return transformTokens(source, CANON_TO_MIRROR);
}

export function fromMirrorIr(source: string): string {
  return transformTokens(source, MIRROR_TO_CANON);
}
