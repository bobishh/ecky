# Delta for authoring-error-surface

## ADDED Requirements

### Requirement: Authoring errors name the failing layer

The system SHALL tag every authoring failure with the layer that owns it —
surface, Core IR, or backend — so the cause is identifiable without external
documentation.

#### Scenario: Surface parse failure

- GIVEN an `.ecky` source with a syntax error
- WHEN the source is parsed
- THEN the error reports layer `surface`
- AND it includes the line span of the offending form.

#### Scenario: Operation absent from the Core IR

- GIVEN a form that is not a known Core IR operation
- WHEN the source is lowered
- THEN the error reports layer `coreIr`
- AND it names the offending operation.

#### Scenario: Operation unsupported by the active backend

- GIVEN an operation that lowers into the Core IR
- AND the active backend cannot execute it
- WHEN the model is rendered
- THEN the error reports layer `backend`
- AND it names the active backend.

### Requirement: Authoring errors offer a fix

The system SHALL attach a structured fix to authoring errors where a concrete
next action or valid alternative exists.

#### Scenario: Unknown operation suggests the nearest valid op

- GIVEN an operation name that is a near-miss of a known Core IR op
- WHEN lowering fails on it
- THEN the error suggestions include the nearest valid operation name.

#### Scenario: Constrained-value error lists the valid set

- GIVEN an argument restricted to a fixed set of values (e.g. an axis symbol)
- WHEN an out-of-set value is supplied
- THEN the error fix lists the valid values.

#### Scenario: Raw message is always preserved

- GIVEN any authoring error with a fix attached
- WHEN it is presented to the user
- THEN the original raw message remains visible
- AND the layer and fix are shown as additional, distinct elements.

### Requirement: Bad input does not panic

The system SHALL return a structured authoring error, not a panic, when a
targeted authoring site receives invalid user input.

#### Scenario: Invalid input on a backfilled site

- GIVEN a backfilled authoring site
- WHEN it receives invalid user input
- THEN it returns an `AppError` carrying a layer
- AND it does not panic.

### Requirement: Error fields are non-breaking on the boundary

The system SHALL add the new error fields without breaking existing consumers.

#### Scenario: Older payload without new fields

- GIVEN an error payload that omits the layer and fix fields
- WHEN it is deserialized
- THEN deserialization succeeds with those fields absent.

### Requirement: Docs no longer carry the debugging band-aid

The system SHALL keep the authoring lesson learnable from errors, so the docs do
not need an up-front error-decoding section.

#### Scenario: Field guide opens on the first model

- GIVEN the Ecky IR field guide
- WHEN it is parsed
- THEN its first section is the first hands-on model
- AND no up-front architecture section precedes it.
