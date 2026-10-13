# Ecky Mode for Emacs

A major mode for editing `.ecky` files in Emacs. Ecky is a Scheme-derived DSL for CAD modeling, and this mode provides syntax highlighting, indentation, and other language support.

## Installation

### Manual Installation

1. Copy `ecky-mode.el` to your Emacs load path, e.g.:
   ```bash
   cp ecky-mode.el ~/.emacs.d/lisp/
   ```

2. Add the following to your init file (e.g. `~/.emacs.d/init.el` or `~/.config/emacs/init.el`):
   ```elisp
   (add-to-list 'load-path "~/.emacs.d/lisp/")
   (require 'ecky-mode)
   ```

### Use-package Installation

If you use `use-package`, add this to your configuration:

```elisp
(use-package ecky-mode
  :load-path "path/to/ecky/editors/emacs"
  :mode ("\\.ecky\\'" . ecky-mode)
  :hook (ecky-mode . ecky-mode-hook))
```

## Features

- **Syntax Highlighting**: Font-lock for all language constructs:
  - Model clauses: `params`, `part`, `meta`
  - Model wrappers: `begin`, `let`, `let*`
  - Expression forms: `define`, `lambda`, `if`, `quote`, `list`, `map`, `filter`, etc.
  - Numeric helpers: `+`, `-`, `*`, `/`, `sin`, `cos`, `clamp`, `lerp`, `hash01`, `noise2`, etc.
  - Point list helpers: `jitter2`, `polar-points`, `organic-loop`, etc.
  - Boolean helpers: `not`, `and`, `or`, `=`, `even?`, `null?`, etc.
  - CAD operations: `box`, `sphere`, `cylinder`, `extrude`, `union`, `difference`, etc.
  - Wall pattern modes: `gyroid`, `schwarz-p`, `ribs`, `rings`, etc.

- **Indentation**: Scheme-derived indentation:
  - 2-space indentation for body forms
  - Special handling for `define`, `lambda`, `let`, and `let*`
  - Proper alignment of bindings and expressions

- **Comments**: Semicolon comments (`;`) to end of line

- **Syntax Table**: Proper parenthesis matching, string delimiters, and symbol characters

- **Imenu Support**: Jump to `define` forms via `M-x imenu`

## Language Elements

### Model Clauses
- `params` — Declares user-visible controls and parameter values
- `part` — Declares named renderable parts
- `meta` — Stores model metadata

### Model Wrappers
- `begin` — Groups multiple model clauses
- `let` — Parallel local bindings
- `let*` — Sequential local bindings

### Expression Forms
- `define` — Defines a helper value or function
- `lambda` — Creates an anonymous function
- `if` — Conditional expression
- `quote` — Literal data (`'value`)
- `list`, `append`, `reverse` — List operations
- `range`, `map`, `filter`, `fold`, `reduce` — Iteration
- `zip`, `enumerate` — Pairing helpers
- `linspace` — Evenly spaced samples
- `flat-map`, `concat-map` — Flat mapping
- `apply` — Call function with list arguments

### Numeric Helpers
- `+`, `-`, `*`, `/` — Arithmetic
- `min`, `max`, `abs`, `floor` — Basic math
- `sin`, `cos`, `tan`, `atan`, `atan2` — Trigonometry
- `deg`, `rad`, `deg->rad`, `rad->deg` — Unit conversion
- `clamp`, `lerp`, `smoothstep` — Interpolation
- `hash01`, `hash-signed`, `noise2`, `fbm2`, `voronoi2`, `cell-distance2` — Procedural noise

### Point List Helpers
- `jitter2`, `jittered-grid` — Grid variations
- `polar-points` — Circular point distributions
- `organic-loop`, `wave-loop` — Organic profiles
- `superellipse-point` — Superellipse sampling
- `voronoi-cells` — Voronoi-ish point centers
- `lorenz-points`, `rossler-points` — Attractor projections
- `logistic-bifurcation-points`, `henon-points` — Chaotic maps

### Boolean Helpers
- `not`, `and`, `or` — Logical operators
- `=`, `>`, `>=`, `<`, `<=` — Comparisons
- `even?`, `odd?`, `zero?` — Number predicates
- `null?`, `empty?`, `list?` — List predicates

### CAD Operations (Portable)
- **Primitives**: `box`, `sphere`, `cylinder`, `cone`, `torus`, `ellipse`
- **2D Profiles**: `circle`, `ring`, `rectangle`, `rounded-rect`, `rounded-polygon`, `polygon`, `regular-polygon`, `trapezoid`, `wedge`
- **Slots**: `slot-overall`, `slot-center-to-center`, `slot-center-point`, `slot-arc`
- **Paths**: `path`, `polyline`, `bezier-path`, `bspline`
- **3D Operations**: `extrude`, `revolve`, `loft`, `sweep`, `taper`, `twist`
- **Modifiers**: `shell`, `offset`, `offset-rounded`, `fillet`, `chamfer`
- **Booleans**: `union`, `fuse`, `difference`, `cut`, `intersection`, `common`, `xor`
- **Transforms**: `translate`, `rotate`, `scale`, `mirror`
- **Arrays**: `linear-array`, `radial-array`, `grid-array`, `arc-array`
- **Repeats**: `repeat`, `repeat-union`, `repeat-compound`, `repeat-pick`, `for-union`, `for-compound`
- **Advanced**: `helical-ridge`, `thread`, `tapped-hole`, `rib`, `groove`, `sampled-radial-loft`
- **Coordinate**: `plane`, `location`, `path-frame`, `place`, `clip-box`
- **Build**: `build`, `shape`, `result`

### CAD Operations (EckyRust-only)
- `mesh` — Indexed triangle geometry
- `polyhedron` — Closed solid from triangles
- `protrude` — Continuous relief from image data
- `wall-pattern` — Procedural wall textures
- `hull` — Convex hull of solids

### Wall Pattern Modes
- `ribs`, `rings`, `spiral`, `diamond`, `hammered`
- `fourier`, `cellular`, `fbm`
- `gyroid`, `schwarz-p`, `schwarz-d`, `diamond-field`, `neovius`, `attractor-field`

## Syntax Highlighting Examples

```scheme
; Comments are semicolon-based
(params (number radius 20 :label "Radius" :min 5 :max 80))

(part body
  (cylinder radius height))

(define wall 2)

(let* ((r 20) (h (* r 3)))
  (part base (cylinder r h)))

(union
  (box 40 20 10)
  (cylinder 12 30))

(wall-pattern (:mode gyroid :depth 0.6 :uFreq 4 :vFreq 5)
  (shell 2 (cylinder 20 80)))
```

## Customization

### Faces

The mode defines these customizable faces:

- `ecky-keyword-face` — Keywords (model clauses, wrappers)
- `ecky-function-face` — Built-in functions and CAD operations
- `ecky-boolean-face` — Boolean literals (`#t`, `#f`)
- `ecky-number-face` — Number literals
- `ecky-string-face` — String literals
- `ecky-comment-face` — Comments
- `ecky-keyword-prefix-face` — Colon keyword prefixes (`:label`, `:mode`, etc.)

Customize them via `M-x customize-group RET ecky RET`.

### Hooks

Use `ecky-mode-hook` to run code when the mode activates:

```elisp
(add-hook 'ecky-mode-hook
          (lambda ()
            (display-line-numbers-mode)
            (aggressive-indent-mode)))
```

## Auto-Mode Association

The mode automatically activates for files ending in `.ecky`. To add additional patterns:

```elisp
(add-to-list 'auto-mode-alist '("\\.ecky\\'" . ecky-mode))
```

## Byte-Compilation

For better performance, byte-compile the mode:

```bash
emacs -batch -f batch-byte-compile ecky-mode.el
```

Or from within Emacs:

```elisp
(byte-compile-file "path/to/ecky-mode.el")
```

## Development

The mode derives from `scheme-mode`; Ecky-specific form heads and literals add
CAD-aware font-lock on top. `npm run test:editors` checks its built-in lists
against `src-tauri/src/ecky_language_surface.rs`.
