# Design: Bottom Icon Dock

## Reference Reading

Use Cultiwars as a structure reference only:

- bottom horizontal action rail.
- compact icon cells.
- grouped controls with separators.
- hover/focus popover affordance.

Do not reuse Cultiwars surface style:

- no paper/cream background.
- no hand-drawn garden icons.
- no rounded or soft toolbar framing.

## Target UI

The workbench owns one bottom overlay rail:

```text
viewport
  -> bottom dock shell
    -> primary group: Projects, Params, Dialogue, Docs, Code, Sketch
    -> utility group: Audio, Terminal when present, Draw, Settings
```

Every control remains a real `<button>`. Visible content is an Ecky icon glyph
and optional tiny caption for scanability. Accessible names stay full:

- Projects
- Parameters
- Dialogue
- Ecky IR docs
- Code inspector
- Sketch Workspace
- Mute/Unmute audio
- Open terminal
- Draw Annotations
- Settings

Behavior rules:

- `CODE` is dock-owned and toggles the code inspector modal on repeated clicks.
- `SKETCH` uses `windowStore` like other floating tools. No fullscreen mode.
- Sketch internal header does not render `CLOSE SKETCH`; window chrome and dock
  toggle own close behavior.
- Export actions keep export/fork only. Source access is not duplicated there.

## Icon Direction

Use deterministic CSS/SVG/Unicode-free text glyphs instead of generated images
or emoji:

- Projects: folder/cards.
- Params: constraint sliders.
- Dialogue: speech bubble.
- Docs: document page with folded corner.
- Code: angular source brackets.
- Sketch: simple geometric construction line with nodes.
- Audio: speaker/radio wave.
- Terminal: `>_` as existing terminal convention.
- Draw: pencil, because annotation mode needs immediate recognition and must
  not duplicate Sketch's geometric node language.
- Settings: cog/gear.

Implementation can use inline SVG snippets or CSS pseudo-elements. First slice
uses inline SVG for primary icons and keeps `>_` for terminal.

## Visual Rules

- Dock position: `absolute`, bottom centered, safe above viewport edge.
- Dock shell: square border, midnight translucent background, backdrop blur.
- No decorative background pattern in the dock shell.
- Active state: `--primary` border and calm green fill.
- Utility separator: 1px vertical bronze/line divider.
- Overflow: hidden on dock shell and groups.
- Mobile: dock wraps or compresses without escaping viewport.

## New Project Rule

The dock does not expose standalone create controls. New project creation remains
inside `ProjectSwitcher.svelte` through its existing `+ NEW` button and global
chooser callback.

## Testing Strategy

Use Playwright from outside-in:

- Given workbench loads, When dock renders, Then it is in the bottom half and
  accessible icon buttons exist.
- Given dock renders, When queried, Then no standalone new-project `+` dock
  button exists.
- Given Projects opens, When `+ NEW` is clicked, Then the global new-project
  chooser opens outside the Projects window.
- Given Projects load error or empty/pending state is present, Then failure or
  pending copy remains visible inside Projects.

Unit tests are not required for pure markup/CSS unless icon metadata is moved
into a helper module.
