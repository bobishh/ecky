# Delta for ecky-language-maturity

## ADDED Requirements

### Requirement: Define Inside Model Fails Early

The `.ecky` compiler SHALL reject `(define ...)` forms inside `(model ...)`
before Steel evaluation, producing an error that names `let*` as the correct
pattern, instead of falling through to Steel's eager evaluator.

#### Scenario: Value define inside model

- GIVEN `.ecky` source with `(define wall 3)` inside `(model ...)`
- WHEN the source is compiled
- THEN compilation fails with an unsupported-feature error
- AND the error message contains `(define ...)` is not supported
- AND the error message contains `let*`

#### Scenario: Param-referencing define inside model

- GIVEN `.ecky` source with `(define half (- frame_length 2))` inside `(model ...)`
- WHEN the source is compiled
- THEN compilation fails before Steel evaluation
- AND the error message does NOT contain `TypeMismatch`
- AND the error message contains `let*`

#### Scenario: Function define at top level still allowed

- GIVEN `.ecky` source with `(define (fn args) body)` outside `(model ...)`
- WHEN the source is compiled
- THEN compilation succeeds

#### Scenario: Literal value define at top level still allowed

- GIVEN `.ecky` source with `(define wall 3)` outside `(model ...)`
- WHEN the source is compiled
- THEN compilation succeeds
