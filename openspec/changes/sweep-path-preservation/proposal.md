## Why

Native sweep currently keeps every second vertex of a linear spine. A three-point
path with one sharp corner therefore becomes a diagonal shortcut. The resulting
solid has wrong bounds and volume. The same path handling must keep round profile
sections intact for the sampled polyline used by Bézier paths.

## What Changes

- Preserve every linear spine vertex when constructing transported sweep sections.
- Use ruled section lofting so authored polyline corners remain separate straight
  spans instead of being smoothed past their path.
- Remove only numerically collinear interior samples, then bound native loft work
  at 1024 remaining linear sections and return an explicit error above that bound.
- Keep the existing 16 linear samples per cubic Bézier segment and its OCCT RTTI
  workaround; sampled curves use the same bounded, round-section sweep path.
- Add native acceptance coverage for sharp, multi-corner, curved round-profile,
  and oversized paths.

## Capabilities

### New Capabilities

- `sweep-path-preservation`: Authored linear spine corners survive native sweep;
  sampled Bézier spines retain circular section geometry under a bounded section
  budget.

### Modified Capabilities

- `native-sweep`: Linear sweep section selection and loft transition semantics.

## Impact

The direct OCCT sweep implementation and its standalone native integration test
change. Long non-collinear paths now fail before loft construction with a precise
section limit error. Collinear samples are removed without changing the centerline.
Bézier approximation remains the existing fixed 16 samples per cubic segment; this
change does not claim analytic Bézier geometry.
