# TODO

- [ ] More preview types (pluggable via `preview::Previewer`): PDF, archives
      (list contents), audio/video metadata, Markdown rendered, CSV as table,
      EXIF next to images.
- [ ] Editor: optional external editor (`$EDITOR` / setting), Shift-F4 new file, warn when the file changed on disk.
- [ ] Icon themes: loadable icon sets (e.g. Zed icon themes), optional per-type colours.
- [ ] Search: show the matching line of content hits (preview could scroll to it); regex option.
- [ ] Windows Quick Access pins in Go to (needs the shell API).
- [ ] Ideas from yazi (terminal file manager):
  - [ ] Yank/cut/paste between panels and tabs, besides F5/F6.
  - [ ] Tabs per panel.
  - [ ] Openers: rules by MIME type/extension choosing which app opens a file
        (incl. "open with…" list).
  - [ ] Richer previews: PDF, video thumbnails, archive listing, scrolling
        and paging inside the preview.
  - [ ] Shell command on selection with output capture (ties into tools).
  - [ ] Lua plugins via `mlua` (features `lua54`, `vendored`: Lua compiled in,
        nothing to install; prototype builds for Linux and Windows). A plugin
        defines tools (`id`, `group`, `label`, `applies(ctx)`, `params`,
        `plan(ctx)` returning an action table) adapted to the `Tool` trait;
        plugins live in `<config dir>/fyle/plugins/*.lua`. Same trust rule as
        tools.toml: only from the user's config.
- [ ] Zed theme import (converter from Zed's theme JSON).
- [ ] More attribute columns (open-ended; add a `ColumnKind` + renderer):
      git status, EXIF data, SELinux context, xattrs, created time.
- [ ] macOS support (later): CI job, app menu, Cmd shortcuts, app bundle.

## Done

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
- [x] Tree mode (toggleable per panel, Ctrl-T).
