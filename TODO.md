# TODO

- [ ] Menus and selection, remaining parts: output/history in Tools; a
      "Clear selection" command (Esc already clears marks). Implement
      missing actions as separate features.
- [ ] Archive follow-ups: nested archives; maybe write support.
- [ ] Additional virtual locations: the provider layer is done
      (`src/remote.rs`: Proton Drive through the official `proton-drive`
      CLI; Google Drive, SFTP, S3 and others through rclone; credentials stay
      with those tools). Still open: UI wiring (background listing with
      cancellation, Locations entries, copy/move/trash jobs, `Cmd`s and
      connection errors in the status line).
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
  - [ ] Richer previews: PDF pages beyond the first, video thumbnails, scrolling
        and paging inside the preview.
  - [ ] Shell command on selection with output capture (ties into tools).
        Unclear to the owner; only "open terminal here" (F9, done) was
        wanted. Ask before starting.
- [ ] macOS support (later): CI job, app menu, Cmd shortcuts, app bundle.

## Done

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
- [x] Previews: PDF first page and info (poppler), EXIF data under images.
- [x] Columns: created time, git status, xattrs, SELinux context.
- [x] Lua plugins (`<config dir>/fyle/plugins/*.lua`) define tools (mlua, Lua 5.4 vendored).
- [x] Search: regex option; content hits show their matching line.
- [x] Tools: conversions (image, CSV) share one "Convert" group.
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
- [x] Tree mode (toggleable per panel, Ctrl-T).

## Later

- Release binary around 100 MB or less, a soft goal: currently 75 MB
  (19 MB gzipped) with the release profile (strip, LTO, one codegen unit)
  and every syntax grammar kept.

- Windows (paused): Quick Access pins in Go to (needs the shell API), CI, cross-checks.

## Dropped

- Zed theme import and icon themes (owner's decision).
