## Why

The library currently writes a component at the model origin and renders it as soon as the user clicks the card action. The user needs to choose its position in the model before saving.

## What Changes

- Open a small floating placement window from built-in, local, and installed source-component cards.
- Preview the selected component in the current scene, pick an initial point on the XY plane, and adjust X, Y, and Z with the viewport arrows or millimetre inputs.
- Apply the position as part of the normal source version workflow. Cancel discards the placement.

## Capabilities

- `component-placement` defines how the desktop library previews and places reusable source components.

## Impact

The library, workbench viewer, and Rust source materializer share the selected component identity and the destination version. The first release supports translation. Port mating and rotation can follow independently.
