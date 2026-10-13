# Properties and inspector tabs

## Implemented scope

View and Properties are sibling tabs in one inspector, which follows the active
folder's cursor. The View menu retains the existing inspector layouts. Alt-Enter
and File › Properties open the inspector on Properties; the command also appears
in the file context menu, command palette and F1 help. F3 opens/selects View,
closes the inspector when View is already active, and switches from Properties
to View. Layout changes retain the selected tab. The tab choice is kept for the
current app session and is not persisted.

Properties reads listing facts for local, remote and archive entries without
touching remote or archive content. For local entries it reports filesystem
attributes, distinguishes symlink objects from targets, and reads supported
EXIF fields for regular image files. Properties has its own inspection result
and session; it does not load `Preview::Content`. Reads run in the background,
are bounded and cancellable, reject stale completions, and preserve listing and
filesystem facts when EXIF is unavailable. The controller permits one blocking
inspection worker at a time and keeps only the newest replacement request
queued until a cancelled worker exits.

Workspace owns layout, active folder and folder roles. `InspectorPane` owns the
selected tab and renders either the Preview or Properties child. `PreviewSession`
continues to own PDF page, workbook sheet and target state. A switch to
Properties invalidates an unfinished full Preview load and any pending page or
sheet result while retaining a completed Preview for the same target; switching
back restarts interrupted work. Closing the inspector clears its Preview target
and releases notebook images. Each feature's request generations reject results
that finish after invalidation.

The Properties model retains the source `Location` separately from its listing
`Entry`. Remote and archive entries show only source listing facts, never
attributes of a downloaded cache copy. Its semantic groups, typed fields,
nested groups and tables provide a shape for future format readers. The initial
view renders listing, filesystem/symlink and EXIF sections. SQLite structure,
Parquet schema/row-group details, and additional PDF/media metadata are not part
of this implementation.

## Remaining work

- Add SQLite structure and Parquet schema/row-group readers as separately
  bounded Properties sections.
- Decide whether any additional metadata should be shared with Preview readers
  when a concrete second consumer exists.
- Validate right and bottom docks at narrow sizes and different UI scales on a
  normal desktop session; headless Xvfb F-key input has known limitations.

## Regression cases

- Rapid selection changes and tab switches reject obsolete results; cancelled
  inspection workers retain the concurrency slot until they actually exit.
- Refresh and editor saves invalidate both inspection perspectives.
- Unknown remote sizes never trigger downloads. Source identities survive any
  future materialization, and broken symlinks retain link and target status.
- Partial failures retain valid facts; truncation and resource bounds remain
  visible. Properties works without optional preview programs.
- Search-hit line reveal, PDF/workbook navigation, notebook resource cleanup,
  folder focus, transfer destinations and inspector layout behavior remain
  intact.
