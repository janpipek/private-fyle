//! Background transfer primitives used by the remote filesystem provider.

use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use remotefs::fs::{ReadOptions, WriteOptions};
use remotefs::{File, RemoteError, RemoteFs};

use super::{CHUNK, remote_error, upload_name};
use crate::remote::Control;

/// Bytes done so far in one provider call, reported through [`Control`].
pub struct Progress<'a, 'b> {
    /// Shared cancellation and progress reporting for the operation.
    pub control: &'a Control<'b>,
    /// Total bytes reported so far in this operation.
    pub done: u64,
}

/// Copies `reader` to `writer` in [`CHUNK`]-sized blocks, stopping with an
/// `Interrupted` error when cancelled and reporting `name` after each block.
pub fn copy_chunks(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    name: &str,
    progress: &mut Progress,
) -> io::Result<()> {
    let mut buffer = vec![0; CHUNK];
    loop {
        if progress.control.cancelled() {
            return Err(Control::cancelled_error());
        }
        let read = match reader.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        writer.write_all(&buffer[..read])?;
        progress.done += read as u64;
        (progress.control.progress)(name, progress.done);
    }
}

/// Transfer context shared by recursive file and folder operations.
pub struct Ctx<'a, 'b, 'c> {
    /// Label used to prefix provider errors.
    pub label: &'a str,
    /// Cancellation and progress state for the operation.
    pub progress: &'a mut Progress<'b, 'c>,
}

impl Ctx<'_, '_, '_> {
    fn err(&self, error: RemoteError) -> io::Error {
        remote_error(self.label, error)
    }
}

/// Downloads `remote` to `local`, skipping anything that already exists.
pub fn download_item(
    fs: &dyn RemoteFs,
    remote: &Path,
    local: &Path,
    is_dir: bool,
    ctx: &mut Ctx,
) -> io::Result<()> {
    if local.symlink_metadata().is_ok() {
        return Ok(());
    }
    if is_dir {
        fs::create_dir(local)?;
        let mut children = fs.list_dir(remote).map_err(|e| ctx.err(e))?;
        children.sort_by_key(File::name);
        for child in children {
            let name = child.name();
            if matches!(name.as_str(), "." | "..") || child.is_symlink() {
                continue;
            }
            crate::location::remote::validate_entry_name(&name)?;
            download_item(fs, child.path(), &local.join(&name), child.is_dir(), ctx)?;
        }
        return Ok(());
    }
    let name = local.file_name().and_then(OsStr::to_str).unwrap_or("file");
    let mut part = local.as_os_str().to_owned();
    part.push(".fyle-part");
    let part = PathBuf::from(part);
    let result = (|| {
        let mut reader = fs
            .open(remote, &ReadOptions::default())
            .map_err(|e| ctx.err(e))?;
        let mut file = fs::File::create(&part)?;
        copy_chunks(&mut reader, &mut file, name, ctx.progress)?;
        file.flush()?;
        reader.finish().map_err(|e| ctx.err(e))?;
        fs::rename(&part, local)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

/// Uploads the local `source` to the server path `remote`, skipping
/// anything that already exists.
pub fn upload_item(
    fs: &dyn RemoteFs,
    source: &Path,
    remote: &Path,
    sftp_names: bool,
    ctx: &mut Ctx,
) -> io::Result<()> {
    if fs.exists(remote).map_err(|e| ctx.err(e))? {
        return Ok(());
    }
    let meta = fs::metadata(source)?;
    if meta.is_dir() {
        fs.create_dir(remote, None).map_err(|e| ctx.err(e))?;
        let mut children = fs::read_dir(source)?.collect::<io::Result<Vec<_>>>()?;
        children.sort_by_key(fs::DirEntry::file_name);
        for child in children {
            if child.file_type()?.is_symlink() {
                continue;
            }
            let local_name = child.file_name();
            let Some(name) = upload_name(&local_name, sftp_names)? else {
                continue;
            };
            upload_item(fs, &child.path(), &remote.join(name), sftp_names, ctx)?;
        }
        return Ok(());
    }
    let name = source
        .file_name()
        .map(|name| upload_name(name, sftp_names))
        .transpose()?
        .flatten()
        .unwrap_or_else(|| "file".to_owned());
    let mut reader = fs::File::open(source)?;
    let options = WriteOptions::default().size_hint(meta.len());
    let mut writer = fs.create(remote, &options).map_err(|e| ctx.err(e))?;
    copy_chunks(&mut reader, &mut writer, &name, ctx.progress)?;
    writer.flush()?;
    writer.finish().map_err(|e| ctx.err(e))
}

#[cfg(test)]
mod tests {
    use super::CHUNK;
    use super::{Progress, copy_chunks};
    use crate::remote::Control;
    use std::io::{self, Cursor};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn copies_in_chunks_with_progress() {
        let data = vec![7u8; CHUNK * 2 + 10];
        let mut out = Vec::new();
        let cancel = AtomicBool::new(false);
        let reports = Mutex::new(Vec::new());
        let record = |name: &str, done: u64| reports.lock().unwrap().push((name.to_owned(), done));
        let control = Control {
            cancel: &cancel,
            progress: &record,
        };
        let mut progress = Progress {
            control: &control,
            done: 0,
        };
        copy_chunks(&mut Cursor::new(&data), &mut out, "f", &mut progress).unwrap();
        assert_eq!(out, data);
        let reports = reports.into_inner().unwrap();
        assert_eq!(
            reports.last().unwrap(),
            &("f".to_owned(), data.len() as u64)
        );
        assert!(reports.len() >= 3);
    }

    #[test]
    fn copy_stops_when_cancelled() {
        let cancel = AtomicBool::new(false);
        let stop = |_: &str, _: u64| cancel.store(true, Ordering::Relaxed);
        let control = Control {
            cancel: &cancel,
            progress: &stop,
        };
        let mut progress = Progress {
            control: &control,
            done: 0,
        };
        let data = vec![1u8; CHUNK * 4];
        let mut out = Vec::new();
        let error = copy_chunks(&mut Cursor::new(&data), &mut out, "f", &mut progress).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(out.len(), CHUNK);
    }
}
