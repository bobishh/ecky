## Decisions

- A bound project supplies `print.orca.json`. Print settings belong to the print job and can be changed independently of the CAD model. The CLI accepts an explicit setup file and the same rendered STL parts.
- The desktop offers one 3MF export. A missing setup produces geometry-only 3MF. A present setup is validated and applied to the export.
- The setup selects bodies by stable part ID, maps each body to a one-based filament slot, and optionally places copies at explicit offsets in millimetres. The exporter centres their combined bounds on the profile's rectangular build plate and places the lowest surface at bed level.
- A referenced Orca project supplies printer, process, and filament settings. Ecky reads its project settings and builds a fresh plate from the selected geometry. Source project geometry, per-object overrides, and existing layer commands are excluded from the new plate.
- Each typed layer event identifies the first layer affected, with layer 1 as the first printed layer. The exporter converts this index to the Z height using the profile's first and subsequent layer heights. Pauses use the printer's configured pause and switches use an existing filament slot.
- Whole-layer switches support one-filament-per-layer jobs on a single nozzle. Multipart jobs can use per-body slot assignments. Ecky validates that events fit the exported height, and Orca remains responsible for slicing and generating G-code.

## Example setup

```json
{
  "schemaVersion": 1,
  "profile": "printer-profile.3mf",
  "parts": [{ "partId": "board", "filament": 1 }],
  "layerEvents": [
    { "type": "pause", "beforeLayer": 18 },
    { "type": "filamentChange", "beforeLayer": 28, "filament": 2 }
  ]
}
```
