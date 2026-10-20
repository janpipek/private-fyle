---
title: "Previews and properties"
weight: 50
---

Press **F3** to show or hide the View inspector. It follows the active panel's
cursor without replacing the other folder panel. Supported previews include
images, text and Markdown, PDF, audio and video, documents, presentations,
notebooks, tables, archives and directories. Readers apply format-specific
limits. PDF viewing needs Poppler; audio and video features need FFmpeg tools.

{{< shot src="images/fyle-preview.png" alt="Fyle showing two folders and a Markdown file in the View inspector" caption="A Markdown file in the View inspector, beside both folder panels." >}}

Press **Alt-Enter** to show Properties for the cursor entry. Local files can
show filesystem facts and format metadata such as image EXIF, audio tags, or
package details. Linux extended attributes can also be inspected. Properties
for remote and archive entries show facts already present in their listings;
they do not download or extract files to inspect them.

Remote files can be quick-viewed after download only when their size is known
and no larger than 8 MiB. Opening a remote file downloads a local cached copy.
Changes made to that copy are not uploaded back to the remote.

The inspector text can be selected and copied with Ctrl-C. In Properties, text
selection takes precedence over the panel's file-copy shortcut.
