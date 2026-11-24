## Why

Changing a numeric dimension in the workbench needs immediate visual feedback while the saved model remains available for comparison and export. A draft can share the production CAD operations and use cheaper display output.

## What Changes

- Show a translucent draft of the affected geometry after a pause in parameter input.
- Reuse the native CAD planner and its geometry cache. Produce a coarse viewport mesh through a dedicated render path.
- Keep Apply on the full version and export workflow. Cancel restores the saved values and clears the preview.

## Capabilities

- `parameter-ghost-preview` defines draft parameter feedback for saved Ecky models that use the native Direct OCCT path.

## Impact

Parameters, the workbench viewer, and the native runner share one draft preview contract. The same separation between display and production output can support a later change to render ordinary scenes cheaply and materialize production artifacts on export.
