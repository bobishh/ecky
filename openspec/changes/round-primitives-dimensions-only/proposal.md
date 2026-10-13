# Round primitives accept dimensions only

Remove the optional segment count from `cylinder`, `cone`, `circle`, and
`ring`. The compiler previously accepted a facet count while native OCCT
constructed circular geometry, misleading authors of captive nut pockets.

This is an immediate language change, without deprecation, warnings, or a
compatibility adapter. Keep polygonal geometry explicit through
`regular-polygon` and `extrude`. Update first-party components, examples,
registry signatures, human documentation, and MCP/API guidance together.
