export type CodeModalSourceAuthority = 'bound' | 'draft';

export async function loadBoundCodeReference<T extends { file: string }>(
  expectedPath: string,
  readSource: () => Promise<T>,
): Promise<T> {
  const source = await readSource();
  if (source.file !== expectedPath) {
    throw new Error(`referenced model no longer matches current thread source. Referenced: ${expectedPath}. Current: ${source.file}.`);
  }
  return source;
}

export function resolveCodeModalSource(input: {
  activeRenderSource?: string;
  boundSource: string;
  activeRenderMatchesViewport: boolean;
}): { source: string; authority: CodeModalSourceAuthority } {
  if (input.activeRenderMatchesViewport && typeof input.activeRenderSource === 'string') {
    return {
      source: input.activeRenderSource,
      authority: 'draft',
    };
  }
  return {
    source: input.boundSource,
    authority: 'bound',
  };
}
