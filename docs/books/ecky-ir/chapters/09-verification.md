## Verification: State What Must Stay True

`verify` turns design assumptions into checks. Author verify clauses from requirements, not from whichever geometry already renders. In MCP flow, treat each clause as an outer TDD test for the model: expect the first run to go red, run `verify_generated_model`, then fix the model and re-render until the same requirement goes green.

Start with the invariant, not the fix. This model says the lid must keep at least `0.3` mm clearance above the body:

```scheme
(model
  (verify
    (tag lid_clearance body.lid_gap)
    (metric gap (clearance min-distance body lid))
    (expect gap (>= 0.3)))
  (part body (box 80 50 20))
  (part lid
    (translate 0 0 20.4
      (box 78 48 3))))
```

![Rendered output for Verification: State What Must Stay True, example 1](assets/09-verification-01.png)

`tag` names the concern. `metric` measures it. `expect` sets the condition.

### Red to green: lid clearance

Red state: the expected clearance is `0.3`, but the lid sits only `0.2` mm above the body. Run `verify_generated_model` on this version. Expect the first run to go red because the requirement is right and the geometry is wrong.

```text
(model
  (verify
    (tag lid_clearance body.lid_gap)
    (metric gap (clearance min-distance body lid))
    (expect gap (>= 0.3)))
  (part body (box 80 50 20))
  (part lid
    (translate 0 0 20.2
      (box 78 48 3))))
```

Green state: keep the same `verify` block and move the lid to `20.4`. Then fix the model and re-render. Run `verify_generated_model` again. The requirement stays fixed while the model changes to satisfy it.

```text
(part lid
  (translate 0 0 20.4
    (box 78 48 3)))
```

Worked red-to-green loop:

1. Write one `verify` clause from one physical requirement.
2. Run `verify_generated_model` and confirm the failure names the violated promise.
3. Change geometry, parameters, or named constraints. Do not weaken the requirement to get green.
4. Fix the model and re-render.
5. Run `verify_generated_model` again until the original clause passes.

Use verification for:

- minimum clearances
- expected part count
- STL triangle or component checks
- required STEP or preview artifacts

Do not delete a failing verification clause to make a render pass. Fix the model or the stated requirement.

### Reading the result (MCP and UI)

`verify_generated_model` returns one check per clause, each with a
machine-readable delta — you do not parse the message string:

```text
authoredVerifyChecks:
  - tag: lid_clearance
    status: failed
    stableNodeId: verify:lid_clearance
    metricSource: clearance
    metricKey: min-distance
    comparator: ">="
    expected: { kind: number, value: 0.3 }
    actual:   { kind: number, value: 0.2 }
```

An agent reads `expected` vs `actual` and the `comparator` to know exactly how
far off the model is, then fixes geometry or parameters and re-renders. In the
app, each clause shows as a red or green chip on the version; the chip's
`stableNodeId` (`verify:<tag>`) focuses the matching verify node in the New
Params map, so a red check jumps you straight to the clause that failed.
