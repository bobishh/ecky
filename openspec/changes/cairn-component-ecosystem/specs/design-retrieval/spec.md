## ADDED Requirements

### Requirement: Unified bounded discovery
The system SHALL expose searchable model revisions, component revisions and package exports through a common bounded retrieval interface shared by CLI, MCP and design orchestration.

#### Scenario: Exact candidate detail
- **GIVEN** a search result carrying an immutable reference
- **WHEN** the caller requests detail
- **THEN** the returned source and contract match that exact reference rather than a mutable latest version

### Requirement: Honest hybrid ranking
The system SHALL combine exact/lexical retrieval with configured semantic retrieval using stable rank fusion, enforce compatibility filters and report the actual retrieval mode.

#### Scenario: Semantic-only match
- **GIVEN** valid cached descriptor embeddings and a configured matching model
- **WHEN** an intent query has no literal identifier match
- **THEN** semantically relevant candidates can be returned with semantic match evidence

#### Scenario: Embeddings unavailable
- **GIVEN** a missing or failing embedding connection
- **WHEN** catalog search runs
- **THEN** lexical results remain usable and the response explicitly reports lexical-only retrieval

#### Scenario: Incompatible candidate
- **GIVEN** a semantically similar component violating a requested backend or interface constraint
- **WHEN** candidates are ranked
- **THEN** similarity does not bypass that constraint

### Requirement: Revision-bound embedding cache
The system SHALL bind embeddings to the descriptor digest, model identity and dimensions and SHALL generate them outside normal search/index publication transactions.

#### Scenario: Descriptor or model changes
- **GIVEN** an embedding for an older descriptor or different model
- **WHEN** retrieval reads current candidates
- **THEN** the stale/incompatible vector is excluded until regenerated

### Requirement: Reuse selection in PLAN
The Rust design controller and MCP authoring guidance SHALL require a bounded search-and-inspection step before creating a new reusable geometry unit and SHALL record reuse, adaptation or no suitable match.

#### Scenario: Existing useful component
- **GIVEN** a design intent with a compatible catalog candidate
- **WHEN** PLAN selects its next build action
- **THEN** it inspects the exact contract/source and records the chosen reuse mode and reference

#### Scenario: Unchanged repair intent
- **GIVEN** unchanged design intent and catalog candidate state during a repair loop
- **WHEN** PLAN is invoked again
- **THEN** previously gathered bounded candidate evidence can be reused instead of repeating broad discovery

### Requirement: Source provenance and untrusted evidence
The system SHALL preserve source provenance and authored license information during adaptation and SHALL treat retrieved text/source as data rather than agent instructions.

#### Scenario: Retrieved instruction-like comment
- **GIVEN** package source containing instruction-like prose
- **WHEN** it enters design context
- **THEN** it remains quoted candidate evidence and does not override controller or user instructions
