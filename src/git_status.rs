//! Git status of a directory's entries, for the Git column. Runs `git
//! status` (off the UI thread) and parses its porcelain output.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
const OUTPUT_LIMIT: usize = 8 * 1024 * 1024;
const STDERR_LIMIT: usize = 64 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Maximum Git status records retained in one parsed snapshot.
pub const STATUS_ITEMS: usize = 100_000;
/// Conservative memory budget for a parsed status index.
pub const STATUS_BYTES: usize = 16 * 1024 * 1024;
const STATUS_ENTRY_OVERHEAD: usize = std::mem::size_of::<(PathBuf, Mark)>() + 16;
const HASH_BUCKET_ESTIMATE: usize = std::mem::size_of::<(PathBuf, Mark)>() + 1;

/// One status result captured for a local listing scope.
#[derive(Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Canonical repository root reported by Git.
    pub root: PathBuf,
    /// Directory whose entries were queried.
    pub scope: PathBuf,
    /// Canonical directory used to map Git's repository-relative paths.
    pub canonical_scope: PathBuf,
    /// Parsed statuses, keyed by absolute native paths.
    pub status: Status,
}

impl Snapshot {
    /// Returns the Git status for an entry expressed under the original listing scope.
    pub fn mark(&self, path: &Path, is_dir: bool) -> Option<Mark> {
        let relative = path.strip_prefix(&self.scope).ok()?;
        self.status
            .mark(&self.canonical_scope.join(relative), is_dir)
    }

    /// Returns a repository-relative selected path when it belongs to this scope.
    pub fn relative_path(&self, path: &Path) -> Option<PathBuf> {
        let relative = path.strip_prefix(&self.scope).ok()?;
        self.canonical_scope
            .join(relative)
            .strip_prefix(&self.root)
            .ok()
            .map(Path::to_path_buf)
    }
}

/// One entry's state, most important last (a directory shows the most
/// important state found inside it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mark {
    Ignored,
    Untracked,
    Renamed,
    Added,
    Deleted,
    Modified,
    Conflicted,
}

impl Mark {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Ignored => "!",
            Self::Untracked => "?",
            Self::Renamed => "R",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Modified => "M",
            Self::Conflicted => "U",
        }
    }

    /// From porcelain v1's two status letters.
    fn parse(xy: &[u8]) -> Option<Self> {
        let (x, y) = (*xy.first()?, *xy.get(1)?);
        Some(match (x, y) {
            (b'!', b'!') => Self::Ignored,
            (b'?', b'?') => Self::Untracked,
            (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => Self::Conflicted,
            (_, b'M') | (b'M', _) | (_, b'T') | (b'T', _) => Self::Modified,
            (_, b'D') | (b'D', _) => Self::Deleted,
            (b'A', _) => Self::Added,
            (b'R', _) | (b'C', _) => Self::Renamed,
            _ => return None,
        })
    }
}

/// Status of the paths under one directory.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// Absolute path → state. Untracked or ignored directories appear as
    /// one entry (git does not list their contents).
    marks: HashMap<PathBuf, Mark>,
}

impl Status {
    /// Parses `git status --porcelain=v1 -z` output; paths are relative to
    /// the repository `root`.
    pub fn parse(root: &Path, output: &[u8]) -> Self {
        let mut marks = HashMap::new();
        let mut fields = output.split(|&b| b == 0);
        while let Some(field) = fields.next() {
            if field.len() < 4 {
                continue;
            }
            let (xy, path) = (&field[..2], &field[3..]);
            if matches!(xy[0], b'R' | b'C') {
                fields.next(); // the original path
            }
            let Some(mark) = Mark::parse(xy) else {
                continue;
            };
            let path = path.strip_suffix(b"/").unwrap_or(path);
            marks.insert(root.join(native_path(path)), mark);
        }
        Self { marks }
    }

    /// Parses complete porcelain records and rejects truncated or malformed output.
    pub fn parse_checked(root: &Path, output: &[u8]) -> io::Result<Self> {
        Self::parse_checked_with_limits(root, output, STATUS_ITEMS, STATUS_BYTES)
    }

    fn parse_checked_with_limits(
        root: &Path,
        output: &[u8],
        max_items: usize,
        max_bytes: usize,
    ) -> io::Result<Self> {
        if !output.is_empty() && output.last() != Some(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Git status output is truncated",
            ));
        }
        let mut fields = output.split(|&byte| byte == 0).peekable();
        let mut items = 0usize;
        let mut path_bytes = 0usize;
        while let Some(field) = fields.next() {
            if field.is_empty() {
                continue;
            }
            if field.len() < 4 || field[2] != b' ' || Mark::parse(&field[..2]).is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Git status output contains a malformed record",
                ));
            }
            if matches!(field[0], b'R' | b'C')
                && fields.next().is_none_or(|original| original.is_empty())
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Git status rename record is incomplete",
                ));
            }
            let path = field[3..].strip_suffix(b"/").unwrap_or(&field[3..]);
            items = items.saturating_add(1);
            path_bytes = path_bytes
                .saturating_add(root.as_os_str().len())
                .saturating_add(path.len());
            if items > max_items {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Git status item limit exceeded",
                ));
            }
            let capacity_estimate = items.saturating_mul(2);
            let bytes_estimate = path_bytes
                .saturating_add(items.saturating_mul(STATUS_ENTRY_OVERHEAD))
                .saturating_add(capacity_estimate.saturating_mul(HASH_BUCKET_ESTIMATE));
            if bytes_estimate > max_bytes {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Git status index byte limit exceeded",
                ));
            }
        }
        Ok(Self::parse(root, output))
    }

    /// The entry's state: its own, or for a directory the most important
    /// state inside it.
    pub fn mark(&self, path: &Path, is_dir: bool) -> Option<Mark> {
        let own = self.marks.get(path).copied();
        if !is_dir || own.is_some() {
            return own;
        }
        self.marks
            .iter()
            .filter(|(p, mark)| **mark != Mark::Ignored && p.starts_with(path))
            .map(|(_, mark)| *mark)
            .max()
    }

    /// Estimates the memory held by the path index for cache accounting.
    pub fn estimated_bytes(&self) -> usize {
        self.marks.iter().fold(
            self.marks
                .capacity()
                .saturating_mul(HASH_BUCKET_ESTIMATE)
                .saturating_add(self.marks.len().saturating_mul(STATUS_ENTRY_OVERHEAD)),
            |total, (path, _)| total.saturating_add(path.as_os_str().len()),
        )
    }
}

#[cfg(unix)]
fn native_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn native_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(unix)]
fn os_string_from_bytes(bytes: Vec<u8>) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes)
}

#[cfg(not(unix))]
fn os_string_from_bytes(bytes: Vec<u8>) -> OsString {
    OsString::from(String::from_utf8_lossy(&bytes).into_owned())
}

/// Discovers and queries Git for one listing scope, enforcing output, time and cancellation bounds.
pub fn discover(dir: &Path, cancelled: impl Fn() -> bool) -> io::Result<Option<Snapshot>> {
    discover_with(dir, OsString::from("git"), cancelled)
}

/// Runs discovery with an explicit executable, allowing deterministic runner tests.
pub fn discover_with(
    dir: &Path,
    executable: OsString,
    cancelled: impl Fn() -> bool,
) -> io::Result<Option<Snapshot>> {
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let root_bytes = match run(
        &executable,
        dir,
        &["rev-parse", "--show-toplevel"],
        &cancelled,
        deadline,
    )? {
        Some(bytes) => bytes,
        None => return Ok(None),
    };
    let root = PathBuf::from(os_string_from_bytes(strip_terminal_lf(root_bytes)));
    let canonical_scope = std::fs::canonicalize(dir)?;
    let scope = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        std::env::current_dir()?.join(dir)
    };
    let relative_scope = canonical_scope.strip_prefix(&root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Git scope is outside repository root",
        )
    })?;
    let scope_arg = if relative_scope.as_os_str().is_empty() {
        OsString::from(".")
    } else {
        relative_scope.as_os_str().to_owned()
    };
    let args = [
        OsString::from("--literal-pathspecs"),
        OsString::from("status"),
        OsString::from("--porcelain=v1"),
        OsString::from("-z"),
        OsString::from("--ignored=matching"),
        OsString::from("--"),
        scope_arg,
    ];
    let result = run_os(&executable, &root, &args, &cancelled, deadline)?;
    if !result.status.success() {
        return Err(process_error("Git status failed", &result.stderr));
    }
    Ok(Some(Snapshot {
        root: root.clone(),
        scope,
        canonical_scope,
        status: Status::parse_checked(&root, &result.stdout)?,
    }))
}

fn strip_terminal_lf(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        #[cfg(windows)]
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    bytes
}

fn run(
    executable: &OsString,
    cwd: &Path,
    args: &[&str],
    cancelled: &impl Fn() -> bool,
    deadline: Instant,
) -> io::Result<Option<Vec<u8>>> {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    let result = run_os(executable, cwd, &args, cancelled, deadline)?;
    if result.status.success() {
        return Ok(Some(result.stdout));
    }
    if result
        .stderr
        .windows(b"not a git repository".len())
        .any(|window| window == b"not a git repository")
    {
        Ok(None)
    } else {
        Err(process_error(
            "Git repository discovery failed",
            &result.stderr,
        ))
    }
}

#[derive(Debug)]
struct ProcessOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[cfg(target_os = "linux")]
fn run_os(
    executable: &OsString,
    cwd: &Path,
    args: &[OsString],
    cancelled: &impl Fn() -> bool,
    deadline: Instant,
) -> io::Result<ProcessOutput> {
    if cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Git query cancelled",
        ));
    }
    let mut command = Command::new(executable);
    command
        .current_dir(cwd)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = spawn_git(&mut command, deadline, cancelled)?;
    let mut stdout = child.stdout.take().expect("stdout was piped");
    let mut stderr = child.stderr.take().expect("stderr was piped");
    #[cfg(target_os = "linux")]
    {
        if let Err(error) = set_nonblocking(&stdout).and_then(|()| set_nonblocking(&stderr)) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    }
    let mut output = Vec::new();
    let mut errors = Vec::new();
    let status = loop {
        if cancelled() || Instant::now() >= deadline {
            let kind = if cancelled() {
                io::ErrorKind::Interrupted
            } else {
                io::ErrorKind::TimedOut
            };
            let message = if kind == io::ErrorKind::Interrupted {
                "Git query cancelled"
            } else {
                "Git query timed out"
            };
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(kind, message));
        }
        if let Err(error) = drain_limited(&mut stdout, &mut output, OUTPUT_LIMIT)
            .and_then(|()| drain_limited(&mut stderr, &mut errors, STDERR_LIMIT))
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    };
    drain_limited(&mut stdout, &mut output, OUTPUT_LIMIT)?;
    drain_limited(&mut stderr, &mut errors, STDERR_LIMIT)?;
    Ok(ProcessOutput {
        status,
        stdout: output,
        stderr: errors,
    })
}

#[cfg(not(target_os = "linux"))]
fn run_os(
    executable: &OsString,
    cwd: &Path,
    args: &[OsString],
    cancelled: &impl Fn() -> bool,
    deadline: Instant,
) -> io::Result<ProcessOutput> {
    if cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Git query cancelled",
        ));
    }
    let mut command = Command::new(executable);
    command
        .current_dir(cwd)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = spawn_git(&mut command, deadline, cancelled)?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let output_reader = std::thread::spawn(move || capture_bounded(stdout, OUTPUT_LIMIT));
    let error_reader = std::thread::spawn(move || capture_bounded(stderr, STDERR_LIMIT));
    let status = loop {
        if cancelled() || Instant::now() >= deadline {
            let kind = if cancelled() {
                io::ErrorKind::Interrupted
            } else {
                io::ErrorKind::TimedOut
            };
            let message = if cancelled() {
                "Git query cancelled"
            } else {
                "Git query timed out"
            };
            let _ = child.kill();
            let _ = child.wait();
            break Err(io::Error::new(kind, message));
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(error);
            }
        }
    };
    let output = output_reader
        .join()
        .map_err(|_| io::Error::other("Git output reader failed"))?;
    let errors = error_reader
        .join()
        .map_err(|_| io::Error::other("Git error reader failed"))?;
    let status = status?;
    let stdout = output?;
    let stderr = errors?;
    Ok(ProcessOutput {
        status,
        stdout,
        stderr,
    })
}

fn spawn_git(
    command: &mut Command,
    deadline: Instant,
    cancelled: &impl Fn() -> bool,
) -> io::Result<std::process::Child> {
    loop {
        if cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Git query cancelled",
            ));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Git query timed out",
            ));
        }
        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(error) if executable_busy(&error) => std::thread::sleep(POLL_INTERVAL),
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn executable_busy(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::ETXTBSY)
}

#[cfg(not(unix))]
fn executable_busy(_error: &io::Error) -> bool {
    false
}

fn drain_limited(
    reader: &mut impl std::io::Read,
    bytes: &mut Vec<u8>,
    limit: usize,
) -> io::Result<()> {
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                if bytes.len().saturating_add(count) > limit {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Git output exceeds size limit",
                    ));
                }
                bytes.extend_from_slice(&chunk[..count]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

fn process_error(prefix: &str, stderr: &[u8]) -> io::Error {
    let message: String = String::from_utf8_lossy(stderr)
        .trim()
        .chars()
        .take(400)
        .collect();
    if message.is_empty() {
        io::Error::other(prefix)
    } else {
        io::Error::other(format!("{prefix}: {message}"))
    }
}

#[cfg(not(target_os = "linux"))]
fn capture_bounded(mut input: impl std::io::Read, limit: usize) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    io::copy(&mut input, &mut io::sink())?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Git output exceeds size limit",
        ));
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn set_nonblocking(file: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    // SAFETY: fcntl reads and updates flags on this live pipe descriptor.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: flags came from this descriptor and O_NONBLOCK only changes its status flags.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_and_aggregates_directories() {
        let root = Path::new("/repo");
        let output = b" M src/main.rs\0?? notes/\0R  new.rs\0old.rs\0A  src/sub/a.rs\0\
                       !! target/\0UU both.rs\0 D gone.txt\0";
        let status = Status::parse(root, output);
        let mark = |rel: &str, is_dir| status.mark(&root.join(rel), is_dir);
        assert_eq!(mark("src/main.rs", false), Some(Mark::Modified));
        assert_eq!(mark("notes", true), Some(Mark::Untracked));
        assert_eq!(mark("new.rs", false), Some(Mark::Renamed));
        assert_eq!(mark("old.rs", false), None);
        assert_eq!(mark("target", true), Some(Mark::Ignored));
        assert_eq!(mark("both.rs", false), Some(Mark::Conflicted));
        assert_eq!(mark("gone.txt", false), Some(Mark::Deleted));
        assert_eq!(mark("clean.rs", false), None);
        // Directories: the most important state inside.
        assert_eq!(mark("src", true), Some(Mark::Modified));
        assert_eq!(mark("src/sub", true), Some(Mark::Added));
        assert_eq!(mark("docs", true), None);
    }

    #[test]
    fn checked_parser_rejects_truncated_and_incomplete_records() {
        let root = Path::new("/repo");
        assert_eq!(
            Status::parse_checked(root, b"?? truncated")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            Status::parse_checked(root, b"R  new\0").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(Status::parse_checked(root, b"?? new\0").is_ok());
    }

    #[test]
    fn checked_parser_enforces_injectable_item_and_index_budgets() {
        let root = Path::new("/repo");
        let one_record = b"?? a\0";
        assert!(Status::parse_checked_with_limits(root, one_record, 1, usize::MAX).is_ok());
        assert_eq!(
            Status::parse_checked_with_limits(root, one_record, 0, usize::MAX)
                .unwrap_err()
                .to_string(),
            "Git status item limit exceeded"
        );
        assert_eq!(
            Status::parse_checked_with_limits(root, one_record, 1, 1)
                .unwrap_err()
                .to_string(),
            "Git status index byte limit exceeded"
        );
    }

    #[test]
    fn preserves_non_utf8_path_bytes_on_unix() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let root = Path::new("/repo");
            let bytes = b"?? bad-\xff-name\0";
            let status = Status::parse(root, bytes);
            let path = root.join(std::ffi::OsStr::from_bytes(b"bad-\xff-name"));
            assert_eq!(status.mark(&path, false), Some(Mark::Untracked));
        }
    }

    #[cfg(unix)]
    #[test]
    fn fake_executable_discovers_root_and_scope_without_git() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        let scope = root.join("nested");
        std::fs::create_dir_all(&scope).unwrap();
        let executable = tmp.path().join("fake-git");
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = rev-parse ] || [ \"$2\" = rev-parse ]; then printf '%s\\n' '{}'; else printf ' M nested/file\\0?? nested/new/\\0'; fi\n",
            root.display()
        );
        std::fs::write(&executable, script).unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();
        let snapshot = discover_with(&scope, executable.into_os_string(), || false)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.root, root);
        assert_eq!(snapshot.scope, scope);
        assert_eq!(
            snapshot.status.mark(&root.join("nested/file"), false),
            Some(Mark::Modified)
        );
        assert_eq!(
            snapshot.status.mark(&root.join("nested/new"), true),
            Some(Mark::Untracked)
        );
    }

    #[cfg(unix)]
    #[test]
    fn discovery_distinguishes_non_repository_from_other_failures() {
        let tmp = tempfile::tempdir().unwrap();
        let executable = tmp.path().join("fake-git");
        write_executable(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' 'fatal: not a git repository (or any parent): .git' >&2\nexit 128\n",
        );
        assert!(
            discover_with(tmp.path(), executable.clone().into_os_string(), || false)
                .unwrap()
                .is_none()
        );

        write_executable(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' 'fatal: detected dubious ownership in repository' >&2\nexit 128\n",
        );
        let error = discover_with(tmp.path(), executable.into_os_string(), || false).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Git repository discovery failed: fatal: detected dubious ownership in repository"
        );
    }

    #[test]
    fn strips_only_the_terminal_newline_from_root_output() {
        assert_eq!(
            strip_terminal_lf(b"/repo/with space \n".to_vec()),
            b"/repo/with space "
        );
        #[cfg(unix)]
        assert_eq!(
            strip_terminal_lf(b"/repo/ending-cr\r\n".to_vec()),
            b"/repo/ending-cr\r"
        );
        #[cfg(windows)]
        assert_eq!(strip_terminal_lf(b"C:\\repo\r\n".to_vec()), b"C:\\repo");
    }

    #[cfg(unix)]
    #[test]
    fn literal_pathspec_and_renamed_whitespace_paths_are_preserved() {
        let root = Path::new("/repo");
        let output = b"R  renamed with space\0old name\0";
        let status = Status::parse(root, output);
        assert_eq!(
            status.mark(&root.join("renamed with space"), false),
            Some(Mark::Renamed)
        );
        assert_eq!(status.mark(&root.join("old name"), false), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn runner_returns_after_child_exit_when_descendant_holds_pipe() {
        let tmp = tempfile::tempdir().unwrap();
        let executable = tmp.path().join("fake-git");
        write_executable(&executable, "#!/bin/sh\n(sleep 0.5) &\nexit 0\n");
        let started = Instant::now();
        let result = run_os(
            &executable.clone().into_os_string(),
            tmp.path(),
            &[],
            &|| false,
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
        assert!(result.status.success());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn runner_kills_and_reaps_on_deadline_and_output_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let executable = tmp.path().join("fake-git");
        let pid_path = tmp.path().join("child.pid");
        write_executable(
            &executable,
            &format!(
                "#!/bin/sh\necho $$ > '{}'\nexec sleep 10\n",
                pid_path.display()
            ),
        );
        let error = run_os(
            &executable.clone().into_os_string(),
            tmp.path(),
            &[],
            &|| false,
            Instant::now() + Duration::from_millis(50),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        let pid: libc::pid_t = std::fs::read_to_string(pid_path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // SAFETY: signal zero only checks whether this child PID still exists.
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));

        write_executable(
            &executable,
            "#!/bin/sh\nprintf '%10000000s' x\nexec sleep 10\n",
        );
        let error = run_os(
            &executable.into_os_string(),
            tmp.path(),
            &[],
            &|| false,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn runner_cancellation_kills_and_reaps_child() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let tmp = tempfile::tempdir().unwrap();
        let executable = tmp.path().join("fake-git");
        let pid_path = tmp.path().join("child.pid");
        write_executable(
            &executable,
            &format!(
                "#!/bin/sh\necho $$ > '{}'\nexec sleep 10\n",
                pid_path.display()
            ),
        );
        let cancelled = AtomicBool::new(false);
        let error = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(50));
                cancelled.store(true, Ordering::Relaxed);
            });
            run_os(
                &executable.into_os_string(),
                tmp.path(),
                &[],
                &|| cancelled.load(Ordering::Relaxed),
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap_err()
        });
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        let pid: libc::pid_t = std::fs::read_to_string(pid_path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // SAFETY: signal zero only checks whether this child PID still exists.
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[cfg(unix)]
    fn write_executable(path: &Path, contents: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, contents).unwrap();
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }
}
