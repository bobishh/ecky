# Preserve parameters in sequence-authored CAD

The spoon-rest source uses pure helpers and `flat-map` over parameter-dependent
`range` values. The symbolic compiler rejects these valid list sources, then
runtime fallback evaluates symbolic parameters as numbers and masks the actual
failure. Replacing controls with constants conceals the defect.

Keep deferred list concatenation in existing Core map/apply nodes, evaluate it
with current native parameter values, and retain editable dimensions in the
existing model. No new authoring lifecycle or external mesh source is introduced.
