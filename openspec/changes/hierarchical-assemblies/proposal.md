## Why

Large models (500–1500+ lines of `.ecky` source) are currently presented as a single flat assembly with an overwhelming global parameter list. Users cannot isolate and edit individual subcomponents (e.g. bracket, motor mount, PCB enclosure) in the 3D viewport without the visual clutter of the entire assembly. Standard CAD systems (Fusion, SolidWorks, FreeCAD) solve this with hierarchical assembly trees, subcomponent isolation (Solo/Isolate mode), and in-context reference ghosting.

## What Changes

- Introduce an **Assembly Tree** navigation panel in the workbench dock showing hierarchical parts and component calls derived from `(part ...)` and `(define-component ...)`.
- Add **Component Viewport Isolation (Solo Mode)**: Selecting a subcomponent isolates its mesh in the 3D viewport, dims/ghosts the rest of the assembly, and zooms/centers camera to its local bounding box.
- Provide **In-Context Parameter Scoping**: When a subcomponent is isolated, the parameter panel filters to show only the parameters belonging to that component.
- Support **Modular Decomposition**: Allow packaging subcomponents into dedicated modules via `cairn` packages or sibling `.ecky` files, keeping top-level `model.ecky` clean and declarative.

## Capabilities

- `hierarchical-assemblies` introduces assembly tree inspection, part isolation in the 3D viewport, and parameter scoping.

## Impact

- `src/lib/types/domain.ts`: Add `AssemblyNode` hierarchy to `ModelManifest`.
- `src/lib/workbench/Model3DViewer.svelte`: Add part isolation and ghosting controls per `partKey`.
- `src/lib/components/`: Add Assembly Tree viewer with isolate/visibility toggles.
- `src-tauri/src/topology_target_ids.rs`: Map part instances to tree nodes.
