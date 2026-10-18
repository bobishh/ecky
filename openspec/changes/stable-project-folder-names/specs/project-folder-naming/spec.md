## ADDED Requirements

### Requirement: Blank projects use stable neutral names

The system SHALL create blank/default-title projects in `ecky-design-<id>`
folders, using the existing thread UUID suffix. Supplied initial titles SHALL
retain Unicode letters and digits. Subsequent title or default-root changes
SHALL NOT move an established folder.

#### Scenario: Codex supplies a later title

- GIVEN a blank project in `ecky-design-<id>`
- WHEN Codex or the user supplies a meaningful conversation title
- THEN the bound source folder remains unchanged

#### Scenario: Initial title contains Cyrillic

- GIVEN an initial title containing Cyrillic letters
- WHEN the project is created
- THEN its initial folder prefix preserves those letters
- AND normal source binding and export operations accept the folder name

### Requirement: Legacy placeholder migration is recoverable

The system SHALL migrate exact legacy `untitled-design-<id>` bindings to
`ecky-design-<id>` under the original parent before starting source consumers.
It SHALL preserve all files, source digests, manifest bytes, history, native
conversation identities and queued prompts. Matching provider working-directory
bindings SHALL move together and require fresh bootstrap instructions.

#### Scenario: Legacy folder contains pending external edits

- GIVEN a legacy bound folder containing changed source and additional files
- WHEN startup migrates its name
- THEN the complete directory moves without changing its contents or digest
- AND the source and matching provider bindings reference the moved directory
- AND another startup makes no further move

#### Scenario: Destination already exists

- GIVEN the desired destination exists, even as an empty directory
- WHEN migration runs
- THEN neither directory is overwritten
- AND the original binding remains usable with an actionable warning

#### Scenario: Application exits between directory move and database commit

- GIVEN a durable relocation intent and a moved owned directory
- WHEN the next startup recovers the operation
- THEN it updates all matching bindings without recreating old source
- AND source consumers start only after recovery succeeds

#### Scenario: Ownership cannot be established

- GIVEN a foreign manifest, symlinked directory, or missing source
- WHEN migration examines the binding
- THEN it refuses movement and reports the concrete error
