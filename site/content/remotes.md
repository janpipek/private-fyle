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

## Add a server

Open **Tools > Manage Remotes**, choose **Add**, select the protocol and fill
in its fields. Give it a name such as `nas`; after saving, open `nas:/` with
{{< key "Ctrl" "L" >}} or choose it in Locations. **Test connection** checks the
connection without opening it. Save changes before choosing **Connect** to open it.

You can also edit `~/.config/fyle/remotes.toml` (or
`$XDG_CONFIG_HOME/fyle/remotes.toml`) directly. Restart fyle or reopen Locations
to discover external changes. Each server has its own `[[remote]]` section:

```toml
[[remote]]
name = "nas"
protocol = "sftp"
host = "nas.example.com"
port = 2222
user = "jan"
root = "/srv/data"
# Omit this when using SSH keys:
password_command = "pass show fyle/nas"
```

`root` defaults to `/`; it is the server folder shown as `nas:/`.

### SFTP: keys and first connection

SFTP uses SSH authentication, including ssh-agent and `IdentityFile` entries
in `~/.ssh/config`. The `host` can be an SSH config alias; its `HostName` and
`Port` are used, with an explicit remote `port` taking precedence. Omit the
password command when authenticating with a key, or supply it for password
authentication.

When a server key is missing from `~/.ssh/known_hosts`, opening the remote or
using **Test connection** shows a confirmation with the resolved host, port, key type and
SHA256 fingerprint. Verify that fingerprint with the server administrator,
then choose **Accept and connect**. Fyle saves the key in `~/.ssh/known_hosts`
and retries. **Cancel** leaves the key untrusted. A changed key is refused;
fyle does not offer to overwrite it. Investigate the change with the server
administrator before updating your known-hosts file.

You can also establish trust in a terminal:

```sh
ssh -p 2222 jan@nas.example.com
```

Verify and accept the fingerprint, then retry in fyle. For an SSH config alias,
use `ssh nas` (and `-p` if the remote overrides its configured port).

### Password command

The **Password command** field contains a program and arguments that print
only the password to standard output. Fyle runs it when making a connection,
including reconnecting. It removes trailing CR/LF characters; the remaining
output is the complete password, including any other lines. An empty result,
failed command or 60-second timeout fails the connection. There is no terminal
input for interactive prompts, so unlock your password manager first if needed.

Examples for the dialog:

```text
pass show fyle/nas
secret-tool lookup service nas.example.com user jan
```

The equivalent TOML entries are:

```toml
password_command = "pass show fyle/nas"
# Or, for a secret stored in the desktop keyring:
# password_command = "secret-tool lookup service nas.example.com user jan"
```

Use a password-manager entry containing only the password. For example, a
`pass` entry containing additional notes would send those notes as part of the
password. Install the chosen program separately and make sure it is available
on the `PATH` used to launch fyle.

Single or double quotes group arguments containing spaces:

```toml
password_command = "pass show 'fyle/my nas'"
```

Commands run directly, without a shell. Pipes, redirection, `$VARIABLE` and
`~` expansion do not work. For custom logic, put it in an executable script
and use its absolute path as the command. Keep secrets out of the command
itself: the command is saved as plain text in `remotes.toml`.

### FTP and FTPS

Use `host`, optional `port` (default 21), `user` and a password command.
Both use passive mode. FTPS enables TLS; plain FTP sends credentials and file
data without encryption. Without a user, fyle uses `anonymous`; without a
password command it supplies an empty password.

```toml
[[remote]]
name = "files"
protocol = "ftps"  # Use "ftp" only for a server requiring plain FTP.
host = "ftp.example.com"
user = "jan"
password_command = "secret-tool lookup service ftp.example.com user jan"
```

### WebDAV

Use `url` instead of `host`. A user and password command supply HTTP basic
authentication; a password command without a user supplies a bearer token.
Omitting both uses no authentication. Use an HTTPS URL for encrypted transport.

```toml
[[remote]]
name = "cloud"
protocol = "webdav"
url = "https://cloud.example.com/remote.php/dav/files/jan"
user = "jan"
password_command = "pass show fyle/cloud"
```

### S3

Use `bucket`, with optional `region`, `profile` and `endpoint`. Credentials
come from the AWS environment or profile, rather than a password command.
A custom endpoint enables path-style addressing for services such as MinIO.

```toml
[[remote]]
name = "photos"
protocol = "s3"
bucket = "my-photos"
region = "eu-west-1"
profile = "default"
# endpoint = "https://minio.example.com:9000"
```

Protocol support depends on the build. The default build includes SFTP and
FTP/FTPS; WebDAV and S3 require the `webdav` and `s3` Cargo features. A build
without the selected protocol reports that when connecting.

## Work with remote files

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
