# Preserve parameters in sequence-authored CAD

The spoon-rest source uses pure helpers and `flat-map` over parameter-dependent
`range` values. The symbolic compiler rejects these valid list sources, then
runtime fallback evaluates symbolic parameters as numbers and masks the actual
failure. Replacing controls with constants conceals the defect.

Keep deferred list concatenation in existing Core map/apply nodes, evaluate it
with current native parameter values, and retain editable dimensions in the
existing model. No new authoring lifecycle or external mesh source is introduced.

Compiler regressions also cover list-valued `zip`/`map`/helper/`append` paths and
diagnostic fidelity. Sequence consumers must keep two- and three-number `(list ...)`
forms as lists without changing point interpretation in geometry contexts. When
expanded compilation and runtime fallback both fail, preserve the actionable
expanded diagnostic instead of replacing it with symbolic arithmetic evaluation.
Quoted tuple destructuring remains unsupported and must explain the supported
`zip`/static `enumerate` forms. The separate live TPU experiment remains read-only;
its `(apply loft ...)` call omits the documented required loft distance.
