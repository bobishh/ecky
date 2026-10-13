# Ecky Language Support for Visual Studio Code

Syntax highlighting and language support for the Ecky CAD DSL.

## Features

- Syntax highlighting for Ecky CAD files (`.ecky`)
- Support for all language constructs:
  - Comments (`;`)
  - Escaped strings
  - Booleans (`#t`, `#f`)
  - Signed decimal and scientific numbers
  - Keywords (`:keyword`)
  - Model clauses (`params`, `part`, `meta`)
  - Model wrappers (`begin`, `let`, `let*`)
  - Expression forms (`define`, `lambda`, `if`, `map`, `filter`, etc.)
  - Numeric helpers (`+`, `-`, `*`, `/`, `sin`, `cos`, `lerp`, `noise2`, `fbm2`, etc.)
  - Point list helpers (`jitter2`, `polar-points`, `organic-loop`, `lorenz-points`, etc.)
  - Boolean helpers (`not`, `and`, `or`, `=`, `>`, `<`, `even?`, etc.)
  - CAD operations (`box`, `sphere`, `cylinder`, `loft`, `extrude`, `union`, etc.)
  - Wall pattern modes (`ribs`, `rings`, `gyroid`, `schwarz-p`, etc.)

## Installation

### Development install

1. Clone this repository
2. Open `editors/vscode` in VS Code
3. Press `F5` to open an Extension Development Host

### Package a VSIX

```bash
cd editors/vscode
npx @vscode/vsce package
code --install-extension ecky-lang-0.0.1.vsix
```

## Language Overview

Ecky is a Scheme-inspired CAD DSL for describing 3D geometry and manufacturing operations. Native OCCT owns rendering; FreeCAD remains an optional interop path.

### Example

```ecky
(model
  (params
    (number radius 20 :label "Radius" :min 5 :max 80)
    (number height 30 :label "Height" :min 10 :max 100))

  (part body (cylinder radius height))
  (part cap (sphere radius)))
```

## File Extension

- `.ecky` - Ecky CAD DSL files

## Syntax Scopes

The grammar uses the following TextMate scopes:

- `comment.line.semicolon.ecky` - Line comments starting with `;`
- `string.quoted.double.ecky` - Double-quoted strings with escape support
- `constant.character.escape.ecky` - String escape sequences
- `constant.language.boolean.true.ecky` - `#t` literal
- `constant.language.boolean.false.ecky` - `#f` literal
- `constant.numeric.ecky` - Numbers (decimal, signed, scientific)
- `keyword.other.ecky` - Keywords like `:label`, `:min`
- `entity.name.tag.ecky` - Keyword name part
- `keyword.control.model-clause.ecky` - Model clauses: `params`, `part`, `meta`
- `keyword.control.model-wrapper.ecky` - Model wrappers: `begin`, `let`, `let*`
- `keyword.control.expression.ecky` - Expression forms: `define`, `lambda`, `if`, etc.
- `support.function.numeric.ecky` - Numeric helper functions
- `support.function.point-list.ecky` - Point list helper functions
- `support.function.boolean.ecky` - Boolean helper functions
- `support.function.cad.ecky` - CAD operation functions
- `support.constant.wall-pattern-mode.ecky` - Wall pattern modes

## TextMate Compatibility

Import `syntaxes/ecky.tmLanguage.json` directly in other
TextMate-compatible editors, including:

- Sublime Text
- TextMate
- JetBrains IDEs (with TextMate bundle support)
