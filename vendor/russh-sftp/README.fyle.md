# Local `russh-sftp` patch

This directory is the upstream `russh-sftp` 2.4.0 crate with one opt-in
compatibility feature, `lossless-strings`. The upstream source, manifest,
README, examples, benchmarks, and Apache-2.0 license are retained. The upstream
repository is <https://github.com/AspectUnk/russh-sftp>.

The local source delta also renames the deprecated atomic `fetch_update` call
to `try_update`, the equivalent API in the active Rust toolchain.

With the feature enabled, protocol strings are converted reversibly between
wire bytes and valid UTF-8 `String`s. A literal `~` becomes `~x7E`; invalid
bytes from `0x80` through `0xFF` become `~xHH` using uppercase hexadecimal.
Other valid UTF-8 is preserved. Serialization decodes only those exact
uppercase markers for `~` and high bytes. It never decodes low-byte markers,
so path separators, NUL, dots, and dot-dot remain ordinary structural bytes.
SFTP `DATA` payloads use byte serialization and are unchanged.

This feature intentionally changes the representation of SFTP protocol text
when callers inspect strings: escaped names are visible as such. Callers that
provide literal path/name text must escape literal tildes with
`wire_text::encode` before passing text to SFTP APIs. Paths returned by SFTP
are already escaped and must be passed back unchanged. This distinction keeps
literal names that resemble escape markers separate from invalid-byte names.

To rebase the patch, replace this directory with the corresponding upstream
2.4.0 source and reapply the `lossless-strings` feature, the `wire_text`
module, the conditional decoder in `src/buf.rs`, the conditional string
serializer in `src/ser.rs`, and the atomic method rename in
`src/client/rawsession.rs`. Keep this note and the upstream license alongside
the source.
