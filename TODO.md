# TODO

- [ ] Discoverable menus (priority): a visible menu bar so users can see what
      fyle can do, ahead of tabs and thumbnails.
  - File: Open, Open with…, New file/folder, Rename, Copy/Move to other panel,
    Trash, Quit.
  - Edit: Cut, Copy, Paste, Select all, Invert selection, Select by pattern.
  - View: Preview, Tree, Hidden files, Columns, Sort, Theme; add List/Icon view
    when icon view is available.
  - Go: Parent, Go to…, Bookmarks, Terminal here; add Back/Forward when available.
  - Tools: context-aware actions grouped by purpose, adapting to the active
    directory and selection (repository tasks for code, conversion/resizing
    for images); add output/history when available.
  - Help: Keyboard shortcuts, Configuration, About; F1 opens Help.
  - Show configured shortcuts beside actions and checked states for toggles;
    disable inapplicable actions with an explanation where practical.
  - Right-click file actions are implemented, sharing command definitions,
    names and availability with the menu bar and command palette.
    Implement missing actions as separate features.
- [ ] Copy/move conflict resolution (including paste): when a destination
      already exists, offer Skip, Replace, and Keep both (choose a unique
      name); for directory conflicts, offer Merge. Show source/destination
      details (path, type, size, modified time) to help the user decide.
      Support applying a choice to remaining conflicts and cancelling the
      operation. Directory merging must handle conflicts within the tree;
      file/directory type mismatches need an explicit decision. Never
      overwrite silently.
- [ ] Browse archives as virtual directories in a panel: enter an archive,
      navigate its folders, and return to the containing directory. Start
      with the formats already supported by archive preview (zip, tar,
      tar.gz/tgz). Support selecting entries, previewing files, and extracting
      selected entries or the whole archive to the opposite panel, reusing
      copy conflict resolution. Show the archive path and internal location
      clearly. Keep archive browsing read-only initially; extraction must
      prevent entries from writing outside the chosen destination.
- [ ] Breadcrumb paths per panel: display the current path as clickable
      segments so users can jump directly to any ancestor. Allow switching
      to an editable full path for typing or pasting a location, and copying
      the full path. Support keyboard navigation and long paths; archive
      browsing should clearly distinguish the archive from its internal path.
  - Add a locations icon beside each panel's breadcrumb path, opening a
    grouped list of drives/mounted volumes, desktop/environment favourites
    and places, and user favourites/bookmarks. Reuse the existing Go to/Places
    data; choosing a location navigates that panel. Provide a tooltip and
    keyboard access so the icon's purpose is discoverable.
- [ ] Selection helpers: Select all, Clear selection, Invert selection, and
      Select by filename pattern or extension. Expose them in Edit and the
      command palette, with configurable shortcuts; work in both list and
      icon views. Operate on visible entries, excluding the parent entry.
- [ ] Optional directory sizes: calculate recursive folder sizes and the
      total size of selected entries on demand, in the background with
      cancellation. Do not scan directories automatically during browsing;
      keep normal listing fast. Indicate incomplete results or read errors.
- [ ] Optional icon view per panel, alongside the existing list view:
      a grid of icons with file names, using preview thumbnails for images
      and file-type icons for other entries. Switch through View → List/Icon
      view; each panel chooses its own mode. Support adjustable icon/thumbnail
      size and the same selection, navigation and file actions as list view.
- [ ] Embedded terminal (idea, undecided): alacritty_terminal + PTY rendered
      in gpui as Zed does, vs. libghostty; see discussion.
- [ ] Ideas from yazi (terminal file manager):
  - [ ] Tabs per panel (parked).
  - [ ] Richer previews: PDF pages beyond the first, video thumbnails, scrolling
        and paging inside the preview.
  - [ ] Shell command on selection with output capture (ties into tools).
- [ ] macOS support (later): CI job, app menu, Cmd shortcuts, app bundle.

## Done

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
- [x] Themes: 36 bundled (GPUI Kit), user theme folder, `theme` setting.
- [x] F3 quick preview: text with syntax highlighting, images, directories, hex dump.
- [x] Previews: rendered Markdown, CSV/TSV as a table, zip/tar/tar.gz listings.
- [x] Tree mode (toggleable per panel, Ctrl-T).

## Later

- Windows (paused): Quick Access pins in Go to (needs the shell API), CI, cross-checks.
