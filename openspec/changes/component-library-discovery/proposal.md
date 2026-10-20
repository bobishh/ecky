## Why

The Library Components view lists installed packages only. Saved extracted
components and the shipped standard library are discoverable through MCP but
absent from the workbench, so a populated component library appears empty.

## What Changes

- Include existing header-only component discovery in the Rust library projection.
- Show shipped and saved components alongside installed packages.
- Search component names, descriptions, parameters, tags, and port identifiers.
- Allow explicit refresh after components are saved externally.
- Insert built-in, extracted, and revision-pinned local components into the active
  model through the existing Rust source/version/render service.
- Keep the foreign FreeCAD catalog available for the FreeCAD engine only.
- Index authored named parts and `define-component` declarations after successful version renders,
  preserving immutable thread-scoped revisions and complete local dependencies.
- Read a local component by stable identity and exact revision digest so future
  local tooling can vendor reproducible source without registry infrastructure.

## Capabilities

### Modified Capabilities
- `component-library`: Workbench discovery covers saved and shipped components.

## Impact

Library service projection, Rust render/version boundaries, Tauri types,
LibraryPanel, and focused backend/browser acceptance tests. Automatic indexing
does not mutate model source or create versions. No network registry is added.
