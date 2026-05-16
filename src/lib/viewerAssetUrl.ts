export function resolveViewerAssetUrl(url: string, cacheKey: string | null | undefined = null): string {
  const key = `${cacheKey ?? ''}`.trim();
  if (!key) return url;
  return `${url}${url.includes('?') ? '&' : '?'}v=${encodeURIComponent(key)}`;
}
