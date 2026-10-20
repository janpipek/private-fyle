---
title: "Remote storage"
weight: 80
---

Remote locations appear as `<name>:/`. Open Locations with {{< key "Ctrl" "G" >}}, type a
remote path with {{< key "Ctrl" "L" >}}, or pass it on the command line. Fyle supports
Proton Drive, rclone remotes, and configured SFTP, FTP, FTPS, WebDAV and S3
servers. SMB is not supported.

Rclone remotes use your existing rclone configuration. For Google Drive,
follow the [Google Drive setup guide](https://github.com/janpipek/private-fyle/blob/main/HOWTO.md). Saved SFTP/FTP/WebDAV/S3 server
connections are managed from Locations or Tools > Manage Remotes and stored in
`remotes.toml`. Passwords are not stored there; an optional password command
provides them when connecting. Proton Drive uses the `proton-drive` CLI and
requires you to authenticate with it first.

Remote listings and supported operations run in the background:

- {{< key "F5" >}} copies between a local folder and a remote folder in either direction.
- {{< key "F6" >}} moves within the same remote when that provider supports it.
- {{< key "F7" >}} creates a remote folder.
- {{< key "Shift" "F6" >}} renames one item.
- {{< key "F8" >}} removes an item.

Local-to-remote and remote-to-local moves, and copies between two remotes, are
unavailable. Use copy and then remove separately when needed.

Removal may send an item to the service's trash or delete it permanently;
read the confirmation before proceeding. Provider capabilities differ.

Opening a remote file downloads a local cached copy. Editing it does not upload
changes back. {{< key "F3" >}} quick view is available for files with a known size up to 8
MiB. Open in Browser is available for supported Proton Drive and rclone Drive
locations.

Remote folders do not support:

- Search, Open With or the built-in editor.
- Tools or opening a terminal.
- Clipboard file operations or remote-to-remote copying.
- Thumbnails, calculated folder sizes or tree mode.

Properties shows listing facts
only; it does not download remote data for inspection. {{< key "Ctrl" "R" >}} refreshes the
active remote listing.
