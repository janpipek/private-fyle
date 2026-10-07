# HOWTO

## Google Drive with fyle

Fyle accesses Google Drive through the `rclone` command line program. It does
not mount Drive, and you do not enter your Google password in fyle.

### Install rclone

On Linux, install a current rclone release. Distribution packages can lag behind
upstream; rclone documents its [official install options](https://rclone.org/install/)
and provides this installer:

```sh
sudo -v
curl https://rclone.org/install.sh | sudo bash
rclone version
```

### Configure Google Drive

Start rclone's interactive setup:

```sh
rclone config
```

1. Choose **New remote**, name it `gdrive` (or another name you will remember),
   and select the `drive` storage type.
2. Use your own Google OAuth client ID and client secret when prompted. Rclone's
   shared Google client ID is scheduled to stop working during 2026; create and
   use your own to avoid interruption. Follow rclone's guide to [make a client
   ID](https://rclone.org/drive/#making-your-own-client-id): enable the Drive
   API, set up the OAuth consent screen, and create a **Desktop app** OAuth
   client. The client ID and secret go only into rclone's prompts.
3. Choose the `drive` scope for the full Drive access needed for uploads and
   file operations. Leave the service-account credentials blank for a normal
   personal account.
4. When asked to authenticate in a web browser, choose **Yes**. Sign in to
   Google and approve access in the browser. The Google password is entered
   only on Google's sign-in page; fyle does not receive it. Keep rclone's local
   configuration private because it stores the OAuth credentials.
5. Choose **No** for a personal My Drive. Choose **Yes** only when configuring a
   separate remote for a Shared Drive, then select that drive.
6. Confirm the remote and quit the configuration menu.

These prompts follow rclone's [Google Drive setup guide](https://rclone.org/drive/).
For machines without a browser, use rclone's [headless setup instructions](https://rclone.org/remote_setup/).
If your external OAuth consent app is left in **Testing**, Google grants expire
after a week. For personal use, rclone's [guide](https://rclone.org/drive/#making-your-own-client-id)
explains publishing an unverified app to avoid weekly reauthorization.

Check that rclone can list the remote:

```sh
rclone lsd gdrive:
```

### Open it in fyle

Run fyle with the remote as a panel, for example:

```sh
fyle gdrive:/ /tmp
```

Or open **Locations** with Ctrl-G and choose the entry under **Remotes**. You
can also edit the active panel path with Ctrl-L and enter `gdrive:/` or
`gdrive:/Documents`. Fyle discovers configured remotes in the background. It
needs `rclone` on `PATH` and must run as the same user that configured it.

F5 copies between a local panel and Drive. F7 creates a folder, Shift-F6
renames, F6 moves within Drive, and F8/Delete sends items to Drive's trash.
The `gdrive.shared` entry, when available, is a read-only view of **Shared with
me**; it supports browsing, downloading and opening items in the browser.
Shared Drive configurations appear as their own remotes.

Opening a remote file downloads a local copy into fyle's cache; editing that
copy does not upload changes back. Native Google Docs, Sheets and Slides are
listed with unknown size (shown as —) and downloaded by Enter using rclone's
default [exports](https://rclone.org/drive/#import-export-of-google-documents):
DOCX, XLSX, PPTX and SVG for drawings. F3 quick view requires a known file size
no larger than 8 MiB. If Drive contains duplicate names in one folder, fyle
lists the first and reports the hidden duplicates; rclone's
[`dedupe` command](https://rclone.org/commands/rclone_dedupe/) can resolve them.
