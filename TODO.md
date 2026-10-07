# TODO

- [ ] Menus and selection, remaining parts: output/history in Tools.
      Implement missing actions as separate features.
- [ ] Keymap editor and presets: build on existing `[[keymap]]` overrides
      and unbinding, with selectable fyle, Krusader, Midnight Commander and
      Finder-style presets beneath user overrides. Keep stable command IDs,
      detect conflicts across overlapping panel/global/editor contexts,
      support reset, and update menu, palette, F1 and tooltip shortcuts.
      Save edits with `toml_edit`, preserving comments and other settings.
      Decide the editor presentation and preset coverage; map only supported
      commands, with Finder's Command-key conventions tied to later macOS
      support. References: official Krusader/MC keymaps and Apple shortcuts.
- [ ] Missing `..` in Proton Drive, live check: the code builds `..` for
      every remote subfolder, and at a remote root only when the panel came
      from a local folder (none for `fyle proton:/`). Reproduce with a real
      account; the cursor-placement part is fixed.
- [ ] Open With on Windows and macOS: read the platform's file-type
      associations behind `apps::Catalog` when those platforms resume.
- [ ] Archive follow-ups: nested archives; maybe write support.
- [ ] File inspection: separate Properties and View as two perspectives.
      Properties shows filesystem attributes and format metadata; View (F3)
      shows file content when there is a useful representation. An image has
      EXIF properties and an image view; a SQLite database can offer its
      structure (tables, columns, indexes and relationships) as properties
      without a content viewer. Parquet schema and row-group metadata also
      belong in Properties. Reuse a UI-independent inspection model, load in
      the background with cancellation and bounded reads, and allow either
      perspective independently. Properties command placement/shortcut and
      the presentation of structured metadata remain to be decided.
- [ ] Workspace layout, open design: keep both directory panels visible and
      usable for file operations while an independent inspector shows Preview,
      Properties or another supported perspective. Consider three resizable
      panes with the inspector docked to the right or bottom; evaluate a 2x2
      layout for two inspectors (Preview plus Properties, or one per directory).
      Base layout availability on usable space after UI scaling. Decide
      minimum pane sizes, follow-selection versus pinned-file behavior, and
      focus/shortcut rules that retain an explicit source and destination for
      file commands. Model layout and inspector state independently and test
      focus, resize and pending-load transitions. Decide before implementing.
- [ ] Mouse interaction and drag-and-drop, open design: start with dragging
      selected typed locations between directory panels through existing
      transfer jobs, progress, cancellation and conflict handling. Show the
      proposed Copy/Move operation and destination before dropping, respecting
      provider capabilities and read-only archive content. Add conventional
      pointer selection, modifier-click toggles and selection ranges using
      the selection model and keymap conventions. Plan external application
      drops separately, including native file/URI data, remote materialization,
      archive extraction and permissions. Decide default drop actions and
      modifiers before implementing; test source/destination identity through
      focus changes, cancelled drags and pending loads.
- [ ] More F3 file types: spreadsheets (XLS/XLSX), Word
      documents (DOC/DOCX), and OpenDocument text (ODT). Keep schema, EXIF and
      other format metadata in the Properties perspective above. Decoding
      libraries versus optional external converters, document fidelity, and
      sheet/page navigation remain to be decided; implement formats as
      separate features and keep tests independent of optional programs.
- [ ] Tabular conversions in the Tools Convert group: extend the existing
      CSV conversions to cover CSV, Parquet and Excel (XLS/XLSX) in both
      directions. Reuse format readers with inspection where practical;
      decide sheet selection, column types, headers/delimiters, formula
      handling and supported Excel output formats. Choose conversion
      backends before implementation and implement each conversion as a
      separate feature with the usual output conflict handling.
- [ ] Remote locations, follow-ups (the UI works: Locations, address bar,
      breadcrumbs, background listing, F5/F6/F7/F8, Shift-F6, open and quick
      view of downloaded copies; see README): SMB (needs libsmbclient);
      connections are serialised per remote, so a long transfer blocks listing
      there (a pool would fix it); copy between two remotes; move between local
      and remote (copy then remove); bulk rename and per-byte progress; uploading
      edited cache copies back; search, tools, thumbnails and folder sizes on
      remotes; automatic refresh when a remote changes;
      a web view for WebDAV and a check of the Proton share-ID
      address; per-remote "Shared with me" is read-only on purpose.
- [ ] Embedded terminal (idea, undecided): alacritty_terminal + PTY rendered
      in gpui as Zed does, vs. libghostty. Recommendation so far: none for
      now (F9 opens an external terminal); if wanted later,
      alacritty_terminal (GPL-compatible).
- [ ] Localization: extract user-facing text into translation resources,
      including command labels, menus, dialogs, help, and status/error messages.
      Support system-language detection, a saved language override, and an
      English fallback; handle plural forms and locale-aware dates and numbers.
      Keep command IDs and configuration keys stable, accommodate longer
      translations, and document how to contribute a language. Translation
      tooling and initial target languages remain to be decided.
- [ ] Ideas from yazi (terminal file manager):
  - [ ] Tabs per panel (parked).
  - [ ] Richer previews: video thumbnails and scrolling inside previews.
  - [ ] Shell command on selection with output capture (ties into tools).
        Unclear to the owner; only "open terminal here" (F9, done) was
        wanted. Ask before starting.
- [ ] macOS support (later): CI job, app menu, Cmd shortcuts, app bundle.

## Done

- [x] Remote Go Up / `..` puts the cursor on the folder just left (the
      shared selection rule only handled the remote root before); model tests
      cover `..` and Go Up in a Proton subfolder, a nested subfolder and the root.
- [x] Swap Panels (`SwapPanels`, Ctrl-U, Go menu, palette, F1): `Folder::swap_with`
      exchanges the two panel models (content, cursor, marks, filter, sort,
      history), the view modes follow (and are saved as `left_view` /
      `right_view`), the active side stays active. Background work is bound to
      a panel position, so the command is unavailable (with a reason) while a
      navigation, remote reload, search or size scan runs; metadata and
      thumbnail work is keyed by the listing generation, which the swap
      renews. The F4 editor and F3 preview cover a panel, so they block it too
      (`panels::swap_blocked`).
- [x] Go to Other Panel's Folder (`GoToOtherFolder`, Ctrl-Left / Ctrl-Right,
      Go menu, palette, F1): the active panel navigates to the other panel's
      typed location; search results use their root, a load in flight there is
      followed to its destination (`pending_target`), Places make it
      unavailable with an explanation. The decision lives in `panels.rs`.
- [x] Clear Selection command (`ClearSelection`, Edit menu, palette, F1): unmarks
      the active panel without touching the filter; unavailable ("Nothing is
      marked") when no entry is marked. Esc keeps its filter-then-marks
      behavior and has no extra shortcut.
- [x] Remote bookmarks: Ctrl-D and the location-bar star now work on remote
      folders too (same `Cmd`; still unavailable in archives, search results
      and Places). Bookmarks are typed locations (`Vec<Location>`), stored in
      `config.toml` with the same versioned encoding as history, so local and
      remote entries mix in one list. The Locations picker (Ctrl-G) lists
      remote bookmarks by their `remote:/path` address, without disk checks,
      and opens them through the normal background listing; a bookmark whose
      remote is no longer configured stays listed and reports "no remote named
      X is configured" on the status line when opened.
- [x] Toggle bookmark: Ctrl-D adds or removes the active local folder's
      bookmark (`ToggleBookmark`, one `Cmd` shared by the shortcut, menus,
      palette and F1 help, checked when bookmarked). A star in each panel's
      location bar shows the state and toggles it on click; it is hidden for
      archives, search results and the drive list. The model lives
      in `bookmarks.rs`; the settings file is edited in the background.
- [x] Read-only Jupyter/IPython notebook view (IPYNB): parse bounded nbformat 4
      JSON in Rust and lazily render Markdown, highlighted code, saved text,
      errors and bounded PNG/JPEG outputs in F3. Raster MIME data is preferred
      in mixed bundles; HTML-only bundles render a sole image with a base64 PNG
      or JPEG data URI. Other HTML (including Markdown HTML and raw-cell HTML),
      SVG, math, attachments, widgets, remote Markdown images and unknown
      outputs are placeholders. No Python runtime, execution or editing.
      Kernel/language and cell counts remain Properties work.
- [x] F3 preview for Parquet files: native table samples (500 rows, 40 columns,
      1,024 characters per cell; 8 MiB metadata and a 64 MiB selected column
      chunk decoding budget), with common codecs; repeated/list/map columns report an
      unsupported-format message.
- [x] F3 previews and icon thumbnails for common camera RAW extensions by
      extracting an embedded JPEG preview when available. Bounded prefix scan,
      candidate count, JPEG dimensions and decode memory; source-aware atomic
      extraction cache and freedesktop thumbnails keyed to the original RAW
      file. JPEG extraction is best-effort and does not decode sensor data.
- [x] Make Ctrl-wheel zoom consistently over table rows, headers, blank
      space and the icon grid; keep ordinary wheel input scrolling.
- [x] Split safe archive extraction helpers and their tests into
      `archive/extraction.rs`, keeping the public API available from `archive`.
- [x] Load owner, EXIF, extended-attribute and SELinux table values, plus
      resolved symlink tooltip paths, asynchronously for visible local rows;
      bound and invalidate metadata work by listing generation.
- [x] Persist typed locations losslessly in frequent-directory history and
      local bookmarks; keep legacy history rows and path-string bookmarks
      readable, write history through a unique synced temporary file, and
      serialize background saves while coalescing pending snapshots.
- [x] Preserve unknown file sizes from providers; show unknown values distinctly,
      keep size sorting deterministic, count marked totals as lower bounds, and
      refuse remote quick-view downloads when the size is unknown.
- [x] Keep a typed source location with materialized remote copies, separate
      from the cache path passed to native readers and openers.
- [x] Introduce typed local, remote and archive addresses for panel folders,
      rows, marks, history, activation and remote transfer selections; model
      Directory, Search and Places panel content explicitly; key remote listing
      caches by remote identity. Keep panel path spellings at UI and native
      operation boundaries.
- [x] Remote start paths on the command line, remote folders in Frequent,
      SFTP host aliases from `~/.ssh/config`, Open in Browser, Google Drive
      duplicates and Shared with me (see README, Remote locations).
- [x] Remote locations in the UI: Remotes group in Locations, `remote:/path` in
      the address bar and breadcrumbs, cancellable background listing with a
      32-folder cache per panel, transfers, remove, new folder, rename and open
      through the providers (`commander/remotes.rs`, `remote/jobs.rs`).
- [x] Remove the permanent bottom shortcut strip; keep status messages and
      running jobs, with commands available through menus, F1 and the palette.
- [x] Fix address-bar button crashes: resolve tooltip shortcuts from the
      panel's focus handle, avoiding recursive key-context parsing; select
      the edited path outside the panel update so redraws cannot reborrow it.
- [x] Icon view, stage 2 (`thumbnails.rs`, `thumb_queue.rs`): image cells
      show thumbnails from the shared freedesktop cache (generated in the
      background, 3 at a time, visible rows plus one, 300 kept in memory;
      failures recorded under `fail/fyle/`). Not yet: svg/ico, EXIF
      orientation; video/PDF frames stay under richer previews.
- [x] Icon view per panel, stage 1 (`grid.rs`, `icon_view.rs`): View › List
      View / Icon View (Ctrl-1 / Ctrl-2) and Larger / Smaller Icons (Alt-= /
      Alt--) for the active panel; a lazy grid of file-type icons (link badge,
      marks as outline + check, name over two lines, tooltip with size) with
      arrow/page navigation, same cursor, marks, context menu and commands as
      the list. Saved as `[panels] left_view`, `right_view`, `icon_size`.
- [x] Distinguish symbolic links (list view): the target's file icon plus a
      badge (curved arrow; crossed circle and muted name when broken), a
      tooltip with the target and resolved path, link line in the F3 header.
      `links::LinkState` and `link_badge::badged` are reusable for the icon
      view. Tar archive symlinks are marked too.
- [x] Optional directory sizes: Calculate Sizes (Ctrl-Shift-S) measures marked / cursor /
      all folders in the background (Esc cancels), cached until refresh or
      navigation, used by size sort and the marked-total footer; `≥` marks
      incomplete results. Unavailable inside archives.
- [x] Breadcrumb paths per panel: clickable, keyboard-focusable ancestors,
      Ctrl-L full-path editing, full-path copy, long-path scrolling, and a
      distinct archive/internal path. Each panel also has a Locations picker
      grouped into drives/mounts, desktop places, bookmarks and frequent paths.
- [x] Browse archives (zip, tar, tar.gz/tgz) as read-only virtual directories (`archive.rs`,
      a mode of `Folder`); F3 previews entries; F5 / Shift-F5 extract to the other panel as a
      background job with the copy conflict dialog; entries never escape the destination.
- [x] Archive follow-ups: the first read of an archive runs in the background
      (panel shows "Reading archive…", Esc cancels; cached afterwards), and
      extraction conflicts show `file.zip › inner/name` as the source.
- [x] Copy/move/paste conflict resolution: Skip, Replace, Keep both, Merge
      (directories), apply to remaining, cancel; details of both entries.
- [x] Zoom (Ctrl-+/-/0, Ctrl-wheel; View menu), saved as `font_size`, and a draggable panel divider.
- [x] Right-click file actions in either panel, sharing command labels,
      availability and shortcuts with the menu bar; preserve marks when
      clicking a marked entry.
- [x] Columns… dialog: pick columns from a grouped catalog, incl. single EXIF fields.
- [x] Previews: audio/video details (ffprobe) and a video frame (ffmpeg).
- [x] Search: the preview scrolls to a content hit's matching line.
- [x] Previews: multi-page PDF navigation (poppler), EXIF data under images.
- [x] Columns: created time, git status, xattrs, SELinux context.
- [x] Lua plugins (`<config dir>/fyle/plugins/*.lua`) define tools (mlua, Lua 5.4 vendored).
- [x] Search: regex option; content hits show their matching line.
- [x] Tools: conversions (image, CSV) share one "Convert" group.
- [x] Open With lists the desktop's default and associated applications (Linux: shared-mime-info, desktop entries, mimeapps.list) after the configured openers, with a "(default)" mark.
- [x] Openers ([[opener]] rules by extension/kind/name) and Open with… (Shift-Enter).
- [x] Yank/cut/paste (Ctrl-C/X/V) between panels; paths also on the system clipboard.
- [x] Editor: Shift-F4 new file, overwrite prompt when changed on disk, optional external editor ([editor] command).
- [x] Configurable columns (panels.columns): name, ext, size, modified, permissions, owner.
- [x] Configurable keymap ([[keymap]] in config.toml) and F1 help listing all commands with keys.
- [x] Rename / bulk rename (Shift-F6): one name per line, validated, swap-safe (rename.rs).
- [x] Quick jump: frecency-ranked directory history and bookmarks (Ctrl-G, Ctrl-D).
- [x] Places from the desktop: XDG/known user folders, GTK bookmarks, KDE Places, Windows drives (in Go to, Ctrl-G).
- [x] Background jobs for copy/move/trash/delete with progress bar, current file and cancel (jobs.rs).
- [x] Find files / search contents (Alt-F7) with ripgrep's libraries; streamed results in the panel, cancellable.
- [x] Tools UI (F2): grouped menu, parameter form, pick list, confirmation with command lines, background run, output dialog; user tools.toml.
- [x] Ctrl-P command palette with key bindings (palette.rs model, palette_view.rs).
- [x] Filter as you type (/ or Ctrl-F), kept in the Folder model, cleared on directory change.
- [x] F8 moves to the system trash (trash crate; Recycle Bin on Windows); Shift-F8 deletes permanently.
- [x] Natural sort (file2 < file10), case-insensitive.
- [x] F4 built-in editor (syntax highlighting, atomic save, unsaved-changes prompt).
- [x] File-type icons, mapped by name/extension in `icons.rs`; glyphs from GPUI Kit's default bundle.
- [x] Runtime theme picker with live preview (Ctrl-K Ctrl-T).
- [x] Themes: 17 bundled (GPUI Kit), user theme folder, `theme` setting.
- [x] F3 quick preview: text with syntax highlighting, images, directories, hex dump.
- [x] Previews: rendered Markdown, CSV/TSV as a table, zip/tar/tar.gz listings.
- [x] Previews: visible scrollbars for overflowing text, archive, notebook, table and detail content.
- [x] Tree mode (toggleable per panel, Ctrl-T).

## Later

- Release binary around 100 MB or less, a soft goal: currently 75 MB
  (19 MB gzipped) with the release profile (strip, LTO, one codegen unit)
  and every syntax grammar kept.

- Windows (paused): Quick Access pins in Go to (needs the shell API), CI, cross-checks.

## Dropped

- Zed theme import and icon themes (owner's decision).
