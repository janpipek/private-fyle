//! Cancellable runner for the command-line programs behind remote providers.

use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const MAX_OUTPUT: usize = 64 * 1024 * 1024;
const MAX_ERRORS: usize = 64 * 1024;
const MAX_MESSAGE: usize = 500;

/// A CLI program with the wording used in its error messages.
#[derive(Clone, Debug)]
pub struct Program {
    /// Executable name or path.
    pub program: OsString,
    /// Service name that starts error messages (e.g. "Proton Drive").
    pub service: &'static str,
    /// Shown when the program is not installed.
    pub install_hint: &'static str,
    /// Appended to errors from a failed run (e.g. how to sign in again).
    pub failure_hint: &'static str,
}

impl Program {
    /// Whether the program is found on `PATH`.
    pub fn is_installed(&self) -> bool {
        on_path(&self.program)
    }

    /// Runs the program with `args` and returns its stdout. Kills it when
    /// `cancel` is set (`Interrupted`) or `timeout` passes (`TimedOut`).
    pub fn run(
        &self,
        args: &[&OsStr],
        cancel: &AtomicBool,
        timeout: Option<Duration>,
    ) -> io::Result<Vec<u8>> {
        if cancel.load(Ordering::Relaxed) {
            return Err(self.cancelled());
        }
        let mut child = Command::new(&self.program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    io::Error::new(error.kind(), self.install_hint)
                } else {
                    error
                }
            })?;
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let output = std::thread::spawn(move || capture(stdout, MAX_OUTPUT));
        let errors = std::thread::spawn(move || capture(stderr, MAX_ERRORS));
        let started = Instant::now();
        let status = loop {
            let stop = if cancel.load(Ordering::Relaxed) {
                Some(self.cancelled())
            } else if timeout.is_some_and(|limit| started.elapsed() > limit) {
                Some(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("{} request timed out", self.service),
                ))
            } else {
                None
            };
            if let Some(error) = stop {
                let _ = child.kill();
                let _ = child.wait();
                break Err(error);
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => std::thread::sleep(Duration::from_millis(40)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(error);
                }
            }
        };
        let output = output
            .join()
            .map_err(|_| io::Error::other("cannot read program output"))?;
        let errors = errors
            .join()
            .map_err(|_| io::Error::other("cannot read program errors"))?;
        if !status?.success() {
            let errors = errors.unwrap_or_default();
            let text = String::from_utf8_lossy(&errors);
            let message: String = text.trim().chars().take(MAX_MESSAGE).collect();
            return Err(io::Error::other(format!(
                "{}: {message}. {}",
                self.service, self.failure_hint
            )));
        }
        output
    }

    fn cancelled(&self) -> io::Error {
        io::Error::new(
            io::ErrorKind::Interrupted,
            format!("{} request cancelled", self.service),
        )
    }
}

/// Whether `program` is an executable file in a `PATH` folder (or, with a
/// directory part, at that path).
pub fn on_path(program: &OsStr) -> bool {
    let path = Path::new(program);
    if path.components().count() > 1 {
        return is_executable(path);
    }
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|dir| is_executable(&dir.join(program))))
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn capture(mut input: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    input
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    // Drain excess output so the child cannot block on a full pipe.
    io::copy(&mut input, &mut io::sink())?;
    if bytes.len() > limit {
        return Err(io::Error::other("program output is too large"));
    }
    Ok(bytes)
}

#[cfg(all(test, unix))]
mod tests {
    use super::Program;
    use std::ffi::OsStr;
    use std::io;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    fn program(name: &str) -> Program {
        Program {
            program: name.into(),
            service: "Test",
            install_hint: "install it",
            failure_hint: "try again",
        }
    }

    fn sh(script: &str, cancel: &AtomicBool, timeout: Option<Duration>) -> io::Result<Vec<u8>> {
        program("sh").run(&[OsStr::new("-c"), OsStr::new(script)], cancel, timeout)
    }

    #[test]
    fn returns_stdout() {
        let out = sh("printf hello", &AtomicBool::new(false), None).unwrap();
        assert_eq!(out, b"hello");
        assert!(program("sh").is_installed());
        assert!(!program("no-such-program-xyz").is_installed());
    }

    #[test]
    fn failure_message_has_stderr_and_hint() {
        let error = sh("echo boom >&2; exit 3", &AtomicBool::new(false), None).unwrap_err();
        assert_eq!(error.to_string(), "Test: boom. try again");
    }

    #[test]
    fn cancel_interrupts_a_running_program() {
        let cancel = AtomicBool::new(false);
        let started = Instant::now();
        let error = std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(150));
                cancel.store(true, Ordering::Relaxed);
            });
            sh("sleep 10", &cancel, None).unwrap_err()
        });
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn missing_program_is_not_found_with_hint() {
        let error = program("no-such-program-xyz")
            .run(&[], &AtomicBool::new(false), None)
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(error.to_string(), "install it");
    }

    #[test]
    fn timeout_kills_the_program() {
        let started = Instant::now();
        let error = sh(
            "sleep 10",
            &AtomicBool::new(false),
            Some(Duration::from_millis(100)),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
