# Proposal: Verify Distance Metrics

## Intent

Extend authored `verify` with a real clearance/distance metric family instead
of overloading current manifest/STL counters.

Current authored verification can prove structural counts and topology signals,
but it cannot express fit-critical spacing between authored selectors. This
change adds one bounded distance metric contract that can compare two named
selectors against manifest, correspondence, and mesh evidence already present
in runtime bundles.

## Scope

- Add a bounded authored metric contract for:
  - `(metric clearance (min-distance selectorA selectorB))`
- Resolve selectors against current manifest parts, selection targets,
  correspondence outputs, and viewer edge/face geometry when available.
- Compute minimum distance from resolved geometry evidence.
- Report unresolved selectors and missing evidence as authored verify errors.
- Merge authored distance failures into existing structural verification output.
- Prove the new metric with Rust and e2e tests.

## Out of Scope

- New public result contract fields.
- Arbitrary expression grammar for selectors.
- Full geometric solver or contact/intersection simulation.
- UI authoring widgets beyond existing verify source editing.

## Expected Outcome

Authors can write one explicit clearance check in `.ecky` source and get a
numeric runtime result that fails when two named selectors are too close.
