# Ecky landing deployment

The default build includes no analytics client. A hosting deployment can supply
`ANALYTICS_CLIENT_URL`, `ANALYTICS_PROJECT` and `ANALYTICS_BROWSER_TOKEN` as
Docker build arguments or environment variables for `npm run build`. The client
is an external HTTPS script; only a public browser credential belongs here.

Set `ANALYTICS_ORIGIN` on the container to the exact external origin permitted
for scripts and collection. The nginx entrypoint expands this setting in its
configuration template. Without it, the content policy permits only local
scripts and connections. Keep deployment values outside this repository.

Run `npm run test:unit` for build configuration tests and `npm run build` to
verify the landing bundle.
