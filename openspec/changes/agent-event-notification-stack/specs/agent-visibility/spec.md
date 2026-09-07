# Delta for agent-visibility

## MODIFIED Requirements

### Requirement: Agent actions are visible

The system SHALL show every distinct user-relevant agent lifecycle transition
and outcome in the session activity journal and SHALL present each event through
the Ecky notification queue without silent replacement or overflow loss. Raw
terminal transport output SHALL remain confined to the dedicated terminal
modal.

#### Scenario: Rapid agent actions arrive between polls

- GIVEN an agent emits six ordered lifecycle events inside one second
- WHEN the frontend receives push events and performs cursor catch-up
- THEN activity contains all six events in cursor order with no duplicates
- AND each event is presented by the notification queue.

#### Scenario: Agent changes macro

- GIVEN an agent changes macro source
- WHEN the change reaches the app
- THEN the activity list shows an agent macro change item
- AND the item links to the macro diff.

#### Scenario: Agent changes parameters

- GIVEN an agent changes model parameters
- WHEN the change reaches the app
- THEN the activity list shows an agent parameter change item
- AND the item shows old and new values.

#### Scenario: Agent validation report arrives

- GIVEN an agent reports validation issues
- WHEN the report reaches the app
- THEN the activity list shows validation status
- AND the detail view includes every issue and raw report text.

#### Scenario: Terminal emits transport output

- GIVEN an interactive agent terminal produces stdout or stderr bytes
- WHEN terminal snapshots update
- THEN those bytes remain available in the dedicated terminal modal
- AND the notification stack and general app log do not mirror the raw stream.

## ADDED Requirements

### Requirement: App renders concurrent notifications in one global stack

The system SHALL render notifications from every thread in one app-global,
vertically stacked notification center ordered by global event cursor and SHALL
preserve stable card identity while lifecycle updates arrive. The notification
center SHALL be separate from the `VertexGenie` mascot component.

The center SHALL be positioned at Ecky's former bubble locus and SHALL also
host the current frontend-only interaction card, when present. The application
SHALL NOT render a second bubble or top-right agent toast lane.

#### Scenario: Two threads report distinct actions

- GIVEN one event belongs to the active thread
- AND another event belongs to a background thread
- WHEN the workbench renders the global notification center
- THEN both messages are visible in separate cards one below the other
- AND each card shows thread and actor attribution
- AND the active-thread card is visually emphasized
- AND the background-thread card remains visible with muted treatment
- AND clicking either card opens that exact activity event.

#### Scenario: Active thread changes

- GIVEN cards from multiple threads are visible
- WHEN the user selects a different active thread
- THEN the newly active thread's cards receive active emphasis
- AND previously active cards become muted
- AND no card is added, removed, reordered, or given a new TTL because of the
  selection change.

#### Scenario: Visible capacity is exceeded

- GIVEN four cards are visible and two more events arrive
- WHEN no visible card has expired or been dismissed
- THEN the two new events wait in FIFO order
- AND activity already contains all six events
- AND each waiting event is promoted after capacity becomes available.

#### Scenario: Mascot renders

- GIVEN the notification center has active cards
- WHEN `VertexGenie` renders the mascot
- THEN `VertexGenie` receives no notification or bubble data
- AND the separate notification center owns card rendering and interaction.

#### Scenario: Mascot identity survives a new model version

- GIVEN an active Ecky thread has persisted `genieTraits`
- WHEN preview or render replaces its artifact or version identity
- THEN the mascot keeps the persisted thread seed
- AND a missing persisted trait uses the model-derived seed only as fallback

#### Scenario: Local queue error appears while agent cards exist

- GIVEN agent activity cards already render beside Ecky
- WHEN a local queue submission fails
- THEN the queue error renders as another card in the same visual stack
- AND no legacy speech bubble or second toast lane appears
- AND the local card receives no fabricated backend cursor or activity record.

#### Scenario: Preview verification result has a matching card tone

- GIVEN an active draft preview reports passed structural verification
- WHEN its notification card renders
- THEN the card reports success with a green result border
- AND active-thread emphasis does not replace that result border.

- GIVEN an active draft preview reports a warning or failure
- WHEN its notification card renders
- THEN warning uses a bronze result border
- AND failure uses a red result border.

#### Scenario: Same text belongs to different events

- GIVEN a dismissed event and a later event have identical summaries
- WHEN the later event arrives with a different event ID
- THEN the later event is visible
- AND the earlier dismissal does not suppress it.

### Requirement: Notification lifetime follows event state

The system SHALL expire resolved informational notifications after 8 seconds of
visible time, resolved warnings after 12 seconds, and SHALL keep active,
error, question, and action-required notifications until resolution or explicit
dismissal. Expiry SHALL NOT delete activity history.

#### Scenario: Completed notification expires

- GIVEN an informational agent action reaches resolved state and is visible
- WHEN it accumulates 8 seconds of visible unpaused time
- THEN its card disappears
- AND its activity record and full detail remain inspectable.

#### Scenario: Active action exceeds completed TTL

- GIVEN an agent action remains active for more than 12 seconds
- WHEN no terminal lifecycle event arrives
- THEN its notification remains visible
- AND elapsed time may update without resetting its identity.

#### Scenario: User inspects a notification

- GIVEN a timed notification has not expired
- WHEN pointer hover or keyboard focus enters the card
- THEN its countdown pauses
- AND countdown resumes only after hover and focus leave.

#### Scenario: App is hidden

- GIVEN timed notifications are visible
- WHEN the document becomes hidden
- THEN all notification countdowns pause
- AND hidden time is not charged against their TTL.

#### Scenario: Error requires attention

- GIVEN an agent event fails with a raw backend or provider body
- WHEN the error notification renders
- THEN it remains until resolved or explicitly dismissed
- AND opening it shows the raw body in activity detail.
