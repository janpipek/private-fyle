# Navigation lifecycle extraction

Status: scope ready for step 6b; implementation has not started.

## Problem

Commander holds parallel per-panel arrays for navigation request generations,
pending flags and pending typed destinations. `commander/location.rs` changes
all three when starting, completing or cancelling a listing. Other-folder
navigation, panel swapping, remote reloads and drag/drop consult parts of that
state. Those lifecycle decisions currently require the GUI controller to test.

The first extraction should make those three values one coherent, independently
tested model. It should preserve the existing behavior and scheduling.

## Active paths

| Path | Responsibility to preserve |
| --- | --- |
| `commander/location.rs::load_panel_listing` | Cancels previous listing/reload work, starts a new request, captures the source-folder guard and exposes the chosen destination. A current completion clears pending state before checking whether the source folder still matches. |
| `commander/location.rs::cancel_location_work` | Escape cancels pending navigation on both panels, invalidates request tokens and drops their task handles. Location discovery and remote transfer checks have separate cancellation paths. |
| `commander/navigation.rs::other_panel_target` | Follows the other panel's pending destination in preference to its displayed folder. |
| `commander/navigation.rs::swap_blocked` | Blocks swaps during navigation, remote reloads, transfer checks, search and size scans. |
| `commander/remotes.rs::reload_remote` | Clears its listing cache even when navigation prevents an in-place reload; never applies a reload while navigation owns the panel. |
| `commander/drag_drop.rs::handle_panel_drop` | Rejects drops involving a panel with navigation or reload work in flight; retains snapshot checks. |

Local, remote, archive and Places navigation all use `load_panel_listing` and
`ListingService`. `commander/archives.rs` is not declared in `commander.rs` and
has no active callers. The old loading-placeholder helpers in `model/archives.rs`
and `model/remote.rs` are test-only. They are not integration targets for 6b.

## Step 6b boundary

Introduce `navigation.rs` as a UI-independent module with one `PanelNavigation`
value per panel. It owns:

- A wrapping request generation and opaque request token.
- Whether navigation is pending and its typed `PanelTarget` destination.
- Starting a request, accepting a current completion and cancelling a pending
  request. An obsolete completion must never clear a newer pending destination.

Expose a small API: `begin(target)`, `complete(token)`, `cancel()`,
`is_pending()` and `pending_target()`. Beginning always creates a newer token,
including when the target is unchanged. Completion returns whether the token
belongs to the current pending request. Cancellation invalidates that request
and clears its destination; cancelling idle state does nothing.

Replace Commander's `navigation_generation`, `navigation_pending` and
`pending_target` arrays with two model values. Update the active paths listed
above to use the model. Keep panel-position ownership: swaps remain blocked
while work is pending, and navigation models do not move with panel content.

Retain the existing source-folder `breadcrumbs::NavigationGuard` as a second
acceptance check. Request freshness and folder freshness are different checks;
6b need not relocate or merge that guard. Preserve the ordering where a current
completion clears pending state before a mismatched folder guard rejects it.

## Keep in existing owners

- GPUI task handles, background scheduling, `Arc<AtomicBool>` cancellation
  signals and status/focus updates remain in Commander.
- Remote reload generations and cancellation signals remain separate. Only
  their check for pending navigation changes to the new model API.
- Listing options, fallback policy, provider calls and caches stay in the
  listing service and existing controllers.
- `ListingCommit`, history/selection rules, size invalidation and listing
  installation remain where they are.
- Location discovery, remote discovery, preview materialization, transfer checks,
  search and size scans are outside this extraction.
- Dormant archive code and legacy test-only placeholders are separate cleanup.

Do not change Escape's existing ordering or extend it to cancel standalone remote
reloads. That behavior needs a separate decision. Do not rename `remote_cancel`
or redesign cancellation as part of this slice.

## Validation

Model tests should cover idle state, begin/complete, repeated-target requests,
replacement A → B → A, cancellation, obsolete completion after cancellation,
and independent panel states. Test local, remote, archive and Places targets as
plain values without disk or provider access. Retain the pending destination
after an obsolete completion and clear it after a current success or failure;
the lifecycle model treats both as completion.

Retain existing `panels::other_panel_target`, swap, drag snapshot, listing and
folder-history tests. Run fmt, clippy and the full test suite before committing.
On Linux with scratch config/data, check rapid target changes, Escape during
navigation, following the other panel's pending destination, Back/Forward,
refresh, rejected drops/swaps while loading, and remote mutation completion
while navigation is pending. Record unavailable live checks explicitly.

Acceptance: the three parallel arrays are gone, request lifecycle tests run
without GPUI, and user-visible navigation, errors, cancellation and selection
behavior are preserved. No dependencies or new commands are needed.
