# TODO

- [ ] More preview types (pluggable via `preview::Previewer`): PDF, archives
      (list contents), audio/video metadata, Markdown rendered, CSV as table,
      EXIF next to images.
- [ ] F4 editor: built-in text editor (gpui-component's code editor), or external `$EDITOR`.
- [ ] Icon themes: loadable icon sets (e.g. Zed icon themes), optional per-type colours.
- [ ] Ctrl-P command palette: fuzzy list of all commands with their key bindings.
- [ ] Zed theme import (converter from Zed's theme JSON).
- [ ] Favourite locations from the desktop environment
      (Windows Quick Access / known folders, KDE Places `user-places.xbel`,
      GTK bookmarks `~/.config/gtk-3.0/bookmarks`, XDG user dirs).
- [ ] Extensible attribute columns (open-ended): pluggable per-entry
      attributes shown as optional columns, e.g. EXIF data, SELinux context,
      git status, permissions/owner, xattrs.
- [ ] macOS support (later): CI job, app menu, Cmd shortcuts, app bundle.

## Done

- [x] File-type icons, mapped by name/extension in `icons.rs`; glyphs from GPUI Kit's default bundle.
- [x] Runtime theme picker with live preview (Ctrl-K Ctrl-T).
- [x] Themes: 36 bundled (GPUI Kit), user theme folder, `theme` setting.
- [x] F3 quick preview: text with syntax highlighting, images, directories, hex dump.
- [x] Tree mode (toggleable per panel, Ctrl-T).
