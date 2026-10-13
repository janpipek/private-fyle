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
filesystem facts when a format reader fails. The controller permits one blocking
inspection worker at a time and keeps only the newest replacement request
queued until a cancelled worker exits.

Properties labels, values, paths, tables and errors are selectable across
separate text runs, so a drag can span fields and sections. Ctrl-C copies an
active Properties text selection before the panel file-yank command. The
selection is cleared when the inspected item changes or Properties is left, so
it cannot shadow a later panel file selection.

Workspace owns layout, active folder and folder roles. `InspectorPane` owns the
selected tab and renders either the Preview or Properties child. `PreviewSession`
continues to own PDF page, workbook sheet and target state. A switch to
Properties invalidates an unfinished full Preview load and any pending page or
sheet result while retaining a completed Preview for the same target; switching
back restarts interrupted work. Workbook sheets are read on demand with a
bounded sample, and generation checks discard results made obsolete by a sheet
change. Closing the inspector clears its Preview target and releases notebook
images. Each feature's request generations reject results that finish after
invalidation.

The Properties model retains the source `Location` separately from its listing
`Entry`. Remote and archive entries show only source listing facts, never
attributes of a downloaded cache copy. Its semantic groups, typed fields,
nested groups and tables provide a shape for future format readers. The view
renders listing facts and only relevant, non-empty filesystem, EXIF, and
additional format groups. Unavailable or empty groups are hidden, while
independent failures and truncated results remain visible. Local regular Parquet
files expose schema paths, physical/logical types, definition/repetition levels,
file row count, and per-row-group row and byte counts. This reader uses only the
footer, capped at 8 MiB; schema output is capped at 256 leaf columns, row groups
at 128, and displayed text at 1024 characters. Footer decoding is synchronous
and cannot be interrupted, and decoded metadata may use more memory than its
serialized footer. Remote and archive contents are not read. A symlink to a
regular local Parquet file is inspected through its target while Filesystem
continues to describe the link object.
Local regular SQLite databases show tables, views, columns (including generated
and hidden columns), indexes and foreign keys from read-only, parameterized
schema queries. User rows are never read. The reader limits output to 128
objects, 1,024 detail rows and 1,024 characters per displayed value; a
two-second deadline and cancellation bound each inspection. SQLite's
256-column read limit can cause an unusually wide schema to fail independently
of filesystem facts. Symlinks to regular databases inspect their targets.
Local regular audio and video files show bounded Container, Video, Audio and
Subtitles groups from `ffprobe`. Probe failures leave filesystem and other
format facts visible. The shared reader limits output to 1 MiB, streams to 32,
text values to 1,024 characters, and execution to ten seconds; Properties
passes its cancellation signal. Its requested stream analysis uses a 10 MB
probe size and a five-second analysis window. Preview uses the reader for audio information
and video detection, then extracts a video frame separately. Additional PDF
metadata remains future work. Image
and video-frame View previews contain no metadata overlay; audio retains its
standalone format and tag details.

## Remaining work

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
