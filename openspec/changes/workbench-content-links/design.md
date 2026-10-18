# Design: Content links are application intents

Markdown remains file-backed. The shared Markdown renderer marks published `.ecky`
examples as downloads; website-only postprocessing is removed. A workbench link
router cancels document navigation synchronously before awaiting dialogs or IPC.
The same router handles documentation and provider-answer HTML. Documentation may
attach its own error presenter; the app-level router handles other anchors and
skips already-consumed gestures.

Bundled source and EPUB links resolve to catalog paths and suggested filenames.
Desktop uses the existing gated native save dialog and a single Rust export command.
Rust accepts only the known EPUB path or a simple `.ecky` filename under
`/docs/examples/`. Release exports exact embedded frontend assets, without SPA
index fallback or dependence on a checkout. Development reads Vite's `public` tree.
Cancellation performs no write; missing assets do not truncate a destination.
Browser mode uses explicit download semantics.

HTTP, HTTPS, and mailto links open with the system handler in desktop mode. Relative
documentation page URLs resolve against the public website. Bound `model.ecky:LINE`
references remain typed Code-inspector actions, with exact-path validation and raw
inline errors. In-document anchors and generated blob downloads retain their browser
behavior. Unsupported local/executable links report errors while preserving the app.

Proof covers real-route native-bridge scenarios for source download, cancellation,
raw export failure, Codex external links, and exact bound-source navigation. The
Code inspector is also checked in isolated WebKit, matching the macOS engine.
