## 1. Contract

- [x] 1.1 Add spec coverage for `clearance min-distance` selector resolution.
- [x] 1.2 Add parser/evaluator tests for metric ref args.

## 2. Runtime

- [x] 2.1 Implement selector resolution against part, selection target, correspondence, edge, and face evidence.
- [x] 2.2 Implement minimum-distance calculation and error handling.
- [x] 2.3 Merge distance failures into authored structural verification output.

## 3. Proof

- [x] 3.1 Add Rust tests for pass, fail, and unresolved selector error cases.
- [x] 3.2 Add e2e proof for a real authored clearance check in the docs path.
- [x] 3.3 Run cargo check and relevant e2e after the metric path is green.
- [x] 3.4 Keep the workbench `INSERT VERIFY` starter contextual so two-part Ecky models seed a clearance example while single-part models keep the safe manifest starter.
