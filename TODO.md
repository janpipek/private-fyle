# TODO

- [ ] Embedded terminal (idea, undecided): alacritty_terminal + PTY rendered
      in gpui as Zed does, vs. libghostty; see discussion.
- [ ] Ideas from yazi (terminal file manager):
  - [ ] Tabs per panel (parked).
  - [ ] Richer previews: PDF pages beyond the first, video thumbnails, scrolling
        and paging inside the preview.
  - [ ] Shell command on selection with output capture (ties into tools).
- [ ] macOS support (later): CI job, app menu, Cmd shortcuts, app bundle.

## Done

- [x] EXIF column (photos: when taken, camera).
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
