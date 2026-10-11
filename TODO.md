# TODO

- [ ] Menus and selection, remaining parts: output/history in Tools.
      Implement missing actions as separate features.
- [ ] Add an image tool to set the selected image as the desktop wallpaper.
      Choose the Linux desktop integration and handle unsupported sessions.
- [ ] Keymap presets: selectable fyle, Krusader, Midnight Commander and
      Finder-style presets beneath user overrides. Map only supported
      commands; Finder's Command-key conventions wait for macOS support.
      References: official Krusader/MC keymaps and Apple shortcuts.
- [ ] Missing `..` in Proton Drive, live check: the code builds `..` for
      every remote subfolder, and at a remote root only when the panel came
      from a local folder (none for `fyle proton:/`). Reproduce with a real
      account; the cursor-placement part is fixed.
- [ ] Open With on macOS: read Launch Services file-type associations behind
      `apps::Catalog` when macOS support begins.
- [ ] Open With on Linux, follow-ups: MIME magic sniffing (now by file name
      only, so unmatched files are `application/octet-stream`); offer it on
      downloaded copies of remote files (still refused on remote folders);
      launching a `Terminal=true`
      application is only unit-tested.
- [ ] Recognise file types beyond extensions across icons, previews and Open
      With. Use a shared, bounded detection model that combines known names
      (such as `Justfile` and `CMakeLists.txt`), extensions and content
      signatures where useful: SQLite databases, XMP sidecars, YAML and GPU
      shader sources are examples. Decide how to handle ambiguous text formats
      and remote files before implementation.
- [ ] Idea: grouped files for related assets, such as RAW/JPEG/XMP photos,
      video/subtitle pairs, audio/lyrics pairs and split archives. Decide
      format-aware pairing rules and an optional panel presentation independent
      of tree/list/icon modes. Decide whether actions target one member or
      the group, including copy, move, rename and delete, before using sidecar
      metadata such as XMP colour labels or star ratings.
- [ ] Open With application icons: show each installed app's desktop icon in
      the chooser where available, with a GPUI Kit fallback. Decide how to
      load external app icons under the current default-bundle icon policy.
- [ ] Open With for directories: detect JetBrains IDE project metadata and
      offer the associated installed IDE explicitly. Decide which project
      markers identify each IDE and how to handle shared or ambiguous markers.
- [ ] Classify directory kinds with a shared model so Open With choices,
      Properties and available Tools reflect the directory's contents and
      project metadata. Keep detection bounded and decide how to represent
      directories that match more than one kind.
- [ ] Add a directory space view as an optional panel, in the spirit of
      Baobab or Filelight, showing where disk space is used. Scan in the
      background with cancellation. Consider making it the default F3 view
      for directories in place of the current direct-child summary; decide
      its layout and interaction with the existing panels.
- [ ] Spreadsheet preview for XLS/ODS: calamine builds a dense range before
      sampling, so a sparse sheet declaring a huge range can use a lot of
      memory. XLSX/XLSM/XLSB now stream cells. The 32 MiB file and 256 MiB
      uncompressed caps limit but don't prevent the remaining risk.
- [ ] Make PDF previews scroll continuously through pages instead of requiring
      Previous/Next page navigation. Keep page rendering bounded and load
      pages as they enter view.
- [ ] Show spreadsheet sheet names as tabs in F3, replacing Previous/Next
      sheet navigation while preserving the bounded per-sheet sample.
- [ ] Idea: follow a growing local text file in F3, with an appendable,
      bounded view that pauses auto-scroll when the user scrolls up and handles
      truncation or rotation. Reuse the view for live tool output if suitable.
- [ ] Idea: let tabular previews scroll beyond the 500-row sample without
      loading a full large file. Virtualize visible rows and design bounded,
      incremental fetching separately for CSV, Parquet and spreadsheets.
- [ ] Headless key input: under bare Xvfb (no window manager) keys sent with
      `xdotool --window` are ignored, and plain `xdotool key` after a click
      reaches the app but F-keys misbehave: F1 opened Locations (Alt-F1's
      action, as if Alt were held) and F2, F3 and F4 did nothing, while Down
      and Ctrl-P worked. Not yet known whether only the test setup or real
      sessions are affected; check F3 on a normal desktop first.
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
- [ ] Workspace layout follow-ups: test the current right/bottom inspector
      arrangements across window sizes and UI scaling; decide whether to add
      a two-inspector layout. Properties remains a separate
      inspection perspective; see File inspection above.
- [ ] Mouse drag follow-ups: offer outgoing desktop file data for remote and
      archive entries after background materialization; support nonlocal URI
      data from external applications; consider drag feedback and modifier
      shortcuts for the Copy/Move choice. Check behavior on a full desktop.
- [ ] Legacy Word (DOC) in F3: out of scope for now (no pure-Rust reader);
      decide later whether to add one or an optional external converter.
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
- [ ] Packaging, the proper way: `just rpm` only repackages a binary built
      on the host. Wanted: a source RPM that builds offline from vendored
      crates (`cargo vendor`) in mock or Copr, an own icon and metainfo,
      and packages for Windows (installer) and later macOS.

## Done

- [x] Copy Full Path for the selected entry: native absolute paths locally,
      standard URLs for configured remotes, unavailable for archives and
      providers without a standard URL.

- [x] Copy Name for the entry under the cursor in the right-click menu,
      Edit menu, palette and F1 help. Works for local, remote and archive
      entries; unavailable on the parent row.

- [x] Add Selection and Tools for Selection submenus to the file right-click
      menu. Tools load asynchronously and open their parameter or confirmation
      steps directly; F2 remains the searchable tools dialog.

- [x] Hide the built-in editor from Open With for binary, non-UTF-8 and
      oversized files; check file content in the background so extensionless
      text stays editable and misleading text extensions do not offer it.
- [x] Windows Open With reads registered file-type defaults and OpenWithProgids/
      OpenWithList applications from the registry, with a system-default fallback.

- [x] Workspace layouts: View offers two panels, two panels plus a Preview
      inspector, and one panel plus Preview. F3 toggles the inspector without
      replacing the inactive folder panel; the hidden panel remains the
      destination for copy and move commands. The selected layout is saved to
      `[workspace] layout` and restored at startup.

- [x] Open With accepts local directories through Shift-Enter, the File menu
      and right-click menu; use `inode/directory` associations and matching
      configured openers, with the built-in editor offered only for files.
- [x] Linux Open With honors desktop-entry `OnlyShowIn` and `NotShowIn`
      against the ordered, case-sensitive `XDG_CURRENT_DESKTOP` names.
- [x] Pointer selection and drag-and-drop for list and icon panels: Ctrl-click
      toggles marks, Shift-click selects ranges, and panel drops ask Copy or
      Move before starting existing jobs. Local files drag out to desktop
      applications and native desktop paths drop into panels. Source identity
      survives focus changes, with pending or changed listings rejected.
- [x] Stream XLSX/XLSM/XLSB preview cells into a bounded sample instead of
      allocating the sheet's full declared range; test sparse far-apart cells.
- [x] Markdown preview shows bold and italic in `.md`, DOCX/ODT and notebook
      text on Linux with variable Cantarell or Noto Sans: use an installed
      static-face font for rich text when the theme selects one of those.
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
- [x] F3 previews Word (DOCX) and OpenDocument text (ODT) documents. Pure
      Rust: the zipped XML is parsed with `zip` and `roxmltree` into a
      UI-independent `Document` model (`preview/document.rs`, `docx.rs`,
      `odt.rs`) and shown through the Markdown view. Headings, bold/italic,
      nested lists, tables, page breaks and image placeholders are kept;
      layout is not. XML entries, blocks and characters are bounded.

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
- [x] F3 preview for spreadsheets (XLSX, XLSM, XLS, XLSB, ODS): native
      `calamine` reader, one sheet at a time with Previous/Next navigation,
      500 rows, 40 columns, 1,024 characters per cell, 32 MiB file and 256 MiB
      uncompressed limits; formulas show cached values, dates as ISO text.
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
- [x] In-app editor for fyle key bindings, with conflict checks, disable/reset and comment-preserving config saves.
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
