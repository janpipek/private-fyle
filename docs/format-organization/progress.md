# Provider migration progress and handoff

Read this file first when joining the work, then the
[implementation plan](implementation-plan.md) and the repository's
[AGENTS.md](../../AGENTS.md). The [synthesis](synthesis.md) explains the design
motivation. Update this log in every implementation commit so the working state
can be reconstructed without the chat.

## Current state

- The owner authorized starting the plan after the planning commit `5da8dee`.
- Completed: slice 1, UI-independent contribution contracts and tests.
- Next: slice 2, adapt the existing GPX/FIT reader through Properties and View.
  No implementation agent currently owns slice 2.
- Working roles, requested by the owner: Astra makes architecture decisions;
  Luna implements; the coordinating agent reviews, verifies and documents.
- Application behavior, runtime execution and GUI wiring are unchanged by
  this first slice. Runtime/trust, renderer scope and extension-column sorting
  checkpoints are still pending for their later slices.
- Keep the unrelated, pre-existing untracked `title.png` out of commits.

## How to join

1. Check `git status --short` and the completion record below before editing.
   Others may be working in the same tree; stage only your changes.
2. Pick a task whose dependencies are complete in the implementation plan.
   Record ownership and file boundaries here before working in parallel.
3. Read the contracts and their tests before adding adapters. Format parsing
   stays under `formats/`; feature scheduling and presentation stay with their
   existing owners until the corresponding slice changes them.
4. Record decisions, changed files, verification results, limitations and the
   exact next task in this log. Update README/TODO and commit each completed
   feature after the required checks. Do not push or open a PR.

## Step log

### 2026-10-16 — Start slice 1

- Read the plan, TODO, README architecture, architecture record and existing
  identity, Properties, preview-session and column models.
- Confirmed the clean tracked baseline is planning commit `5da8dee`; only the
  unrelated `title.png` is untracked.
- Assigned architecture review to Astra and implementation to Luna. Luna owns
  `src/data_providers.rs`, `src/data_providers/` and its module declaration in
  `src/main.rs`; the coordinator owns documentation and final validation.
- Bound the initial work to pure models and in-memory sensor/Git examples.
  No extension runtime, dynamic-column UI, filesystem probes or shared cache
  will be added in this slice.
- Architecture decisions and verification are pending; append their outcomes
  below before marking the slice complete.

### 2026-10-16 — Set the slice 1 contract boundaries

Astra supplied these decisions; Luna is implementing them:

- A provider definition bundles fields and separate Properties, column and
  viewer descriptors. A pure validation function checks a collection before it
  is installed; this is not a shared execution registry or reader trait.
- Provider IDs use `builtin.<name>` or `extension.<package>.<provider>`.
  Registration origin is supplied by the host, not trusted from a package.
  Contributions/fields are owner-qualified. Persisted built-in column IDs keep
  their existing spelling separately from the contribution's qualified ID.
- Initially support text and finite nonnegative duration/distance quantities
  with seconds/metres units. Validate both type and unit against field
  definitions. Do not replace native format models with dynamic values.
- Model selected items, symlink objects and symlink targets as distinct
  subjects, retaining original `Location`s and native path bytes.
- Result availability is independent from completeness: a ready value can be
  complete, truncated or a lower bound. One failed contribution does not remove
  a sibling's ready result. Observations retain producer and provenance.
- Request acceptance checks identity, source/context revisions, demand and UI
  generation. Reuse ignores generation only when revisions are known and all
  other demand/identity fields agree. An unknown revision permits delivery to
  its original request but forbids reuse across requests.
- Applicability consumes captured listing/format/context evidence only.
  Initial demand limits describe input bytes and item counts; the model does
  not claim to enforce execution budgets.
- Use a public pure module, following `properties`' existing module precedent,
  so its contracts can be introduced before production adapters without
  suppressing dead-code warnings. No additional dependencies or GUI changes.

Planned evidence: in-memory sensor (all three presentations) and Git (Properties
and column, context-based applicability) examples, plus boundary and stale-work
tests. Architecture review will check the implemented contract before commit.

### 2026-10-16 — Review the initial contract implementation

- Luna added the initial pure module, ID types and main-module declaration.
  These files are under review; their presence is not slice completion.
- The coordinator requested private validated quantity storage, actual typed
  sensor/Git projection examples rather than constructor-only assertions,
  portable temporary roots, native-byte coverage and stronger same-generation
  stale-result tests.
- Astra is reviewing ownership/collision validation, descriptor applicability
  and the request identity rules before the first verification run.
- Prepared the slice 2 handoff: Properties keeps `route_activity` and current
  decimal strings; View keeps its summary formatting and raster/cache behavior.
  GPX-derived values and FIT recorded session totals need provenance captured
  at the parser assignment. Truncated coordinates must not mark FIT's complete
  recorded totals as incomplete. No production adapter is implemented yet.

### 2026-10-16 — Complete and verify slice 1

- Implemented `src/data_providers.rs` and `src/data_providers/identity.rs`,
  exposed through a documented public module in `src/main.rs`. Namespaces have
  exact shapes, preventing overlapping provider ownership. Validation rejects
  duplicate definitions, foreign fields, undeclared bindings, malformed column
  IDs and conflicting Git alias/canonical registrations.
- Sensor contract example projects native typed input into validated
  observations, selects a column value through its field binding (including a
  rebound distance column), and bounds a typed viewer sample through its
  descriptor. Git applies through captured repository context, contributes
  Properties/column results, and does not require a viewer or format match.
- Added coverage for zero/invalid quantities and units, independent failure and
  truncation, distinct link subjects, native non-UTF-8 identity, and generation,
  source, context identity/revision and demand changes. Unknown revisions accept
  their current completion but cannot authorize reuse.
- Astra reviewed the corrected source and approved slice 1 with no remaining
  blockers. Luna implemented all Rust changes; the coordinator reviewed and
  ran final repository checks.
- Verification: `cargo fmt --check` passed;
  `cargo clippy --all-targets -- -D warnings` passed; `cargo test` passed
  **565 tests, zero failures/ignored**. Luna's focused
  `cargo test data_providers::tests -- --nocapture` passed 8 tests; the full run
  also covers the ID module test. `git diff --check` passed.
- Updated README's module map, TODO and the architecture record. No dependency,
  setting, command, production reader or GUI behavior changed. No live UI check
  was needed for this pure-model slice. No Windows build was run; the model
  introduces no platform-specific production code.

## Slice 2 handoff

Start from the commit titled `Define provider contribution contracts` and read
`src/data_providers.rs` plus its inline examples. Run `cargo test data_providers`
to check both contract and ID tests. Existing presentation registries still use
their original paths; the model examples are not installed user extensions.

1. Assign the GPX/FIT adapter files before editing: new
   `src/data_providers/geo.rs`; existing `src/properties/geo.rs`,
   `src/preview/geo.rs`, their registry wiring, and `src/formats/geo.rs` plus
   `geo/gpx.rs` and `geo/fit.rs` for provenance. Keep documentation with the
   coordinator when delegating implementation.
2. Construct one provider-owned Properties/viewer contribution bundle. Make
   both existing adapters consume its definitions/applicability and native
   prepared data; an unused descriptor beside the old paths does not complete
   this slice. Keep native readers independent from presentation modules.
3. Capture numeric provenance at the parser branch: GPX duration/distance are
   inferred; FIT duration uses recorded session totals, while FIT distance can
   be recorded totals or inferred coordinates. Do not guess from the format.
4. Preserve existing fixed read limits and UI formatting. FIT session totals
   can be complete even with truncated geometry; GPX-derived summaries become
   truncated if traversal stopped. Invalid numeric projection must not discard
   unrelated readable facts or a route outline. Preserve absence versus zero.
5. Use honest unknown revisions until a caller supplies captured freshness;
   never manufacture revision tokens from UI generations. Keep Properties'
   cancellation and listing-only nonlocal boundary, and View's current
   materialization/outline behavior. Do not add a shared acquisition cache.
6. Validate output equivalence, session-total precedence, no-GPS data, segment
   gaps, truncation, cancellation and parse failures. Run normal repository
   checks and a scratch-config live UI check for both inspector perspectives.
   Update this log, README/TODO and the completion record before committing.

## Completion and next work

| Slice | State | Evidence / handoff |
| --- | --- | --- |
| 1. Contribution contracts | Complete | Astra approved; fmt/clippy and all 565 tests passed |
| 2. Existing GPX/FIT adapter | Pending | Depends on completed and reviewed contracts |
| 3. Dynamic columns | Pending | Requires sorting checkpoint before implementation |
| 4. Packages and worker | Pending | Requires execution/loading/trust decisions |
| 5. Sensor renderer proof | Pending | Requires renderer scope decision |
| 6. Contextual Git | Pending | New Git Properties behavior is within the authorized proof scope |
| 7. Review proof | Pending | Both sensor and Git cases must work before broader migration |
| 8. Remaining built-ins | Pending | Depends on slice 7 |
