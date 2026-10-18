# Stable project folder names

Blank threads currently create `untitled-design-<id>` folders before a useful
conversation title exists. Use `ecky-design-<id>` instead. The suffix remains
the existing stable thread UUID suffix, not a source digest.

The user selected stable neutral names rather than moving folders whenever
Codex supplies a title. Explicit titles supplied at creation remain supported,
including non-ASCII letters. Later title and default-root changes retain exact
bound paths.

Migrate only exact legacy `untitled-design-<id>` bindings at application startup.
Do not rename custom or previously titled folders. Preserve their original root,
complete contents, immutable history, and provider conversation identities.
