## Level 03: Dovetail Fit

**Mission:** Make one named fit_clearance drive both mating sides of a dovetail rail and channel.

**Clear condition:** Changing fit_clearance widens the channel while the rail stays nominal; no second anonymous offset needs editing.

The shelf needs to slide out for crumb patrol. The rail stays nominal; the channel grows by a named clearance. That is the same fit idea as the mushroom roof plug, now with a polygonal cross-section.

A dovetail is a sliding fit between a male rail and a female channel. The two
parts mate because the channel is slightly larger than the rail, and that
slight difference **is** the fit. Make the difference one named number and the
whole fit becomes editable from one place.

The trap is authoring each side with its own hard-coded offset: the rail at its
nominal size, the channel widened by a magic literal like `0.6`. That works
once, but the moment you want a looser or tighter fit you have to find and edit
two offsets that were never linked. Worse, the two numbers drift apart with
every edit until the parts no longer mate.

The fix is a single named clearance binding shared by both sides:

- the **male** side uses the nominal profile directly;
- the **female** side is the same profile enlarged by the clearance on every
  side (`nominal + 2 * clearance`).

Change the clearance once and only the channel moves — the rail stays nominal,
so the fit changes through one relation instead of two anonymous offsets.

Reuse a proven profile instead of redesigning it. The dovetail rail in the
film-adapter mechanism is already a tested triangular profile; extracting that
profile and its clearance relation into a smaller fixture preserves the fit
math without inventing a second dovetail. The surrounding mechanism (film path,
detents, helicoid) is complexity you can drop; the mating profile and the named
clearance are what you keep.

When the two mating parts are separate exportable solids, keep any
preview-only assembly placement (a rail hovered above a channel for display)
out of the exported geometry. Each part should export as the clean solid it
really is; the assembly view is a diagnostic, not a feature of either part.
