# Change: Preserve the workbench when opening content links

## Why

The app book renders plain website anchors. Opening `.ecky` source can navigate the
main WebView away from the app, destroying Svelte and its event subscriptions.
Provider answers also contain external anchors; both content surfaces need the same
application navigation boundary.

## What Changes

- Route content link gestures through one workbench policy.
- Save bundled source and EPUB through the native save dialog and exact bundled bytes.
- Open public URLs through the system browser and retain bound model references in Code.
- Share source-download semantics with the static website renderer.

## Scope

Documentation and dialogue links, native documentation export, and regression proof.
No workbench layout or provider generation lifecycle changes.
