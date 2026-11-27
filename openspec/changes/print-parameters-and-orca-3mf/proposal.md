## Why

Ecky exports geometry to slicers. A model that encloses inserts or changes filament at a specific height still needs print instructions recreated in Orca after each geometry export.

## What Changes

- Add a project-local print setup for selecting rendered bodies, assigning filament slots, and scheduling layer pauses and filament changes.
- Export an editable Orca 3MF with the selected geometry and a saved Orca printer, filament, and process profile. Support both single-body and multipart models.
- Keep the existing geometry exports and use the same production geometry when exporting an Orca project.

## Capabilities

- `print-parameters` covers print setup files and Orca project export.

## Impact

The desktop 3MF export includes print settings whenever a setup exists. The desktop and CLI share one Rust exporter. Print setup files live beside the model source and can be reused after geometry changes.
