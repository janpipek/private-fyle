# Properties and inspector tabs

## Status and scope

Agreed architectural plan; implementation remains pending. Properties and View
are sibling tabs in one inspector, following the active folder's cursor. A
future third perspective can be another tab in that inspector. Simultaneously
visible inspectors remain a separate workspace-layout decision.

F3 opens and selects View. If the inspector is already open on View, F3 closes
it; if Properties is selected, F3 switches to View. Changing workspace layout
preserves the selected perspective. Use a closed perspective enum initially;
adding a third perspective does not require a plugin registry or a universal
pane framework.

Properties command placement and shortcut, persistence of the selected tab, and
the initial metadata scope still need decisions before their implementation.
Filesystem facts and EXIF are the recommended first inspection slice. SQLite
structure and Parquet schema/row-group metadata illustrate the required data
shape; this plan does not authorize implementing those readers.

## Current readiness

- `workspace.rs` already defines Preview and Properties perspectives, typed
  local/remote/archive selections, request generations and tests rejecting stale
  completions across perspective changes.
- The inspector already has right/bottom docking and feasible-size checks.
  Tabs can share the existing slot without changing folder roles.
- `preview/session.rs` owns preview lifecycle decisions independently of GPUI;
  PDF pages and workbook sheets can remain Preview-specific state.
- Shared entry attributes and `formats/exif.rs` / `formats/raw.rs` provide
  reusable metadata and format-reader foundations.

The seams are not wired through the application yet:

- `commander/view.rs` forces Preview on F3 and layout changes and always routes
  selection to `PreviewPane`.
- `commander/workspace_view.rs` embeds `self.preview` directly in both docks.
- Workspace inspector request/completion state is consumed only in tests;
  actual loads use `PreviewSession`. Resolve that duplicate lifecycle ownership
  before adding Properties.
- There is no independent inspection result. PDF and media metadata is coupled
  to preview loading; Parquet metadata is read internally and discarded after
  producing a row sample.
- Dropping a GUI task does not necessarily stop an already-running blocking
  reader or subprocess. Stale-result rejection and execution cancellation are
  separate requirements.

## Ownership and contracts

Workspace owns layout, active folder and folder roles. An inspector controller
owns the selected perspective and routes the current typed target to the chosen
feature. Each feature session owns its requests, generations, result acceptance
and feature-specific navigation. Remove or repurpose the unused workspace load
lifecycle so each request has one clear owner.

Keep GUI entities, tasks, debounce, focus and rendering resources in controllers
and views. A thin inspector view renders tabs and the selected feature. A thin
Properties view renders inspection data and forwards input.

Define inspection data independently of `Preview::Content`: filesystem facts
and structured metadata sections with stable field identities, typed values,
nested groups and tables. Represent unavailable values, failures and truncation
explicitly. A failed format section must not discard usable filesystem facts.
Readers return semantic data; presentation labels and formatting belong to the
consumer. Existing EXIF values are library-formatted strings and need not all
be reworked into typed values in the first slice.

Carry the original `Location` separately from any local materialized path.
Remote and archive entries can expose available listing facts without downloads
or extraction. Deeper inspection requires an explicit bounded source-access
policy; cached-copy attributes must not be presented as source attributes.
Properties must not depend on a successful content preview.

Use cancellable background execution with explicit read/output, concurrency and
cache limits. Share semantic format readers when there is a concrete second
consumer; never load `Preview::Content` just to obtain Properties. Metadata
inspection must not decode content samples or generate thumbnails unnecessarily.

## Implementation sequence

1. Extract the inspector controller and tab view. Route selection through them,
   resolve lifecycle ownership, and retain Preview navigation and resource
   handling. Test tab transitions and the agreed F3/layout behavior in the model.
2. Define and test the GUI-independent inspection result and Properties session,
   including partial results, generations and invalidation.
3. Implement the approved first readers with cancellation and enforced bounds.
   Start with filesystem facts and EXIF if that scope is approved; specify local,
   remote and archive availability independently.
4. Render Properties and wire commands through `Cmd` and
   `commander::keys::action_for`. Keep menus, palette, F1 help, availability and
   toggle state consistent. Route refresh and editor-save invalidation through
   the inspector instead of directly to Preview. Add persisted settings only
   after the tab-persistence decision, preserving comments and existing keys.
5. Validate both docks and scaling in the running app. Update README behavior
   and architecture and mark only completed TODO work as done.

Use focused commits without new dependencies unless a concrete reader requires
one and its rationale is stated. A third tab should add its model, controller,
view and command wiring without changing folder roles or dock calculations.

## Acceptance and regression coverage

- Properties remains usable without a content viewer or optional preview tools.
- Rapid selection changes and tab switches reject obsolete results; closing the
  inspector cancels pending work and prevents late presentation updates.
- Same-item refresh and editor saves invalidate affected inspection data.
- Per-feature navigation/resource state has an explicit retention policy across
  tab switches; retained data must be invalidated when its target changes.
- Unknown remote sizes do not trigger unbounded downloads. Source identities
  survive materialization; symlink facts distinguish the link and its target,
  including broken links.
- Partial failures preserve valid facts; truncation and resource bounds remain
  visible and testable. Cancelled workers count toward concurrency limits until
  they actually finish.
- Existing search-hit line reveal, PDF/workbook navigation, notebook resource
  cleanup, folder focus and transfer destinations remain correct.
- Before implementation commits run `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, and `cargo test`. Check GUI flows
  using scratch configuration/data directories at right/bottom docks, narrow
  window sizes and different UI scales.
