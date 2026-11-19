/** The hosting deployment may supply an external browser client; source builds omit it. */
export function deploymentAnalytics(environment) {
  const client = environment.ANALYTICS_CLIENT_URL?.trim();
  const project = environment.ANALYTICS_PROJECT?.trim();
  const token = environment.ANALYTICS_BROWSER_TOKEN?.trim();
  if (!client || !project || !token) return [];
  const url = new URL(client);
  if (url.protocol !== 'https:' || url.username || url.password || url.hash) {
    throw new Error('ANALYTICS_CLIENT_URL must be an HTTPS script URL without credentials or fragment');
  }
  return [{ tag: 'script', attrs: {
    defer: true, src: url.href, 'data-project': project, 'data-token': token,
  }, injectTo: 'head' }];
}
