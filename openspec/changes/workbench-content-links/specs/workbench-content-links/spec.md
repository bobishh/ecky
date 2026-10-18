# Workbench content links

## ADDED Requirements

### Requirement: Content links preserve the application document

Documentation and dialogue content SHALL share link routing that cancels main
WebView navigation before asynchronous work. Bound model source references SHALL
open the existing Code inspector. Public URLs SHALL open outside the native app.

#### Scenario: User opens a Codex source reference

- **WHEN** a bound model reference is activated in Dialogue
- **THEN** Code displays that exact bound source and line
- **AND** workbench windows and event subscriptions remain mounted

#### Scenario: User opens an external link from Codex

- **WHEN** a public source URL is activated in a provider answer
- **THEN** the native system opener receives the URL
- **AND** the app neither navigates nor creates an embedded content window

### Requirement: Documentation files use explicit exports

Published `.ecky` source and EPUB links SHALL export their bundled bytes through a
native save dialog, or an explicit browser download in browser mode. Native export
SHALL accept only documentation catalog paths and SHALL NOT use the SPA fallback.

#### Scenario: User downloads book source

- **WHEN** a `.ecky` link in the book is activated and a destination is chosen
- **THEN** that bundled example is saved to the chosen destination
- **AND** the application stays on the same workbench document

#### Scenario: Download is cancelled or fails

- **WHEN** saving is cancelled or an asset/export fails
- **THEN** cancellation writes nothing and failure preserves the raw diagnostic
- **AND** the workbench remains usable
- **AND** a missing asset does not overwrite an existing destination
