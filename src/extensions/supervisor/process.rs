//! Nonblocking Linux child transport, deadlines and complete child cleanup.

use std::collections::VecDeque;
use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::Pending;
use crate::data_providers::{ResultState, Value};
use crate::extensions::broker::SourceBroker;
use crate::extensions::protocol::{self, HostFrame, HostMessage, ReadResult, WorkerMessage};

#[cfg(target_os = "linux")]
mod transport;
#[cfg(target_os = "linux")]
use transport::{
    FrameDecoder, append_host_frame, bounded_reason, drain_diagnostics, drain_protocol,
    set_nonblocking, write_pending,
};

/// Wall-clock limit from child spawn through terminal frame and successful exit.
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(2);
/// Maximum retained helper diagnostics.
pub const STDERR_LIMIT: usize = 64 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Runs one queued extension request through the platform worker transport.
pub fn execute(
    pending: &Pending,
    cancelled: impl Fn() -> bool,
    launcher: &Arc<dyn Fn() -> io::Result<Command> + Send + Sync>,
    fixture_protocol_stderr: bool,
) -> ResultState<Value> {
    #[cfg(target_os = "linux")]
    {
        let result = launcher().and_then(|command| {
            execute_command(pending, cancelled, command, fixture_protocol_stderr)
        });
        result.unwrap_or_else(|error| ResultState::Failed(bounded_reason(&error.to_string())))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (pending, cancelled, launcher, fixture_protocol_stderr);
        ResultState::Failed("extension workers are unsupported on this platform".into())
    }
}

#[cfg(target_os = "linux")]
/// Builds the current executable command for the reserved helper mode.
pub fn production_command() -> io::Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("--fyle-extension-worker-v1");
    Ok(command)
}

#[cfg(not(target_os = "linux"))]
/// Reports that the supervised extension backend is unavailable on this platform.
pub fn production_command() -> io::Result<Command> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "extension workers are unsupported on this platform",
    ))
}

#[cfg(target_os = "linux")]
fn execute_command(
    pending: &Pending,
    cancelled: impl Fn() -> bool,
    mut command: Command,
    fixture_protocol_stderr: bool,
) -> io::Result<ResultState<Value>> {
    if cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "extension request cancelled",
        ));
    }
    let broker = SourceBroker::open(
        &pending.source,
        crate::data_providers::DemandLimits {
            max_bytes: pending.init.demand.max_bytes,
            max_items: pending.init.demand.max_items,
        },
        pending.init.source_handle,
    )?;
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + REQUEST_DEADLINE;
    let result = (|| {
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| invalid("worker stdin pipe missing"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| invalid("worker stdout pipe missing"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| invalid("worker stderr pipe missing"))?;
        set_nonblocking(&stdin)?;
        set_nonblocking(&stdout)?;
        set_nonblocking(&stderr)?;
        let (protocol_pipe, diagnostic_pipe): (Box<dyn PipeRead>, Box<dyn PipeRead>) =
            if fixture_protocol_stderr {
                (Box::new(stderr), Box::new(stdout))
            } else {
                (Box::new(stdout), Box::new(stderr))
            };
        supervise(
            &mut child,
            (protocol_pipe, diagnostic_pipe),
            stdin,
            broker,
            RunContext {
                init: &pending.init,
                deadline,
                cancelled: &cancelled,
            },
        )
    })();
    if result.is_err() {
        // Always attempt cleanup: a failed try_wait must not bypass reaping.
        let _ = child.kill();
    }
    let status = child.wait()?;
    match result {
        Ok(state) if status.success() => Ok(state),
        Ok(_) => Err(invalid("extension helper exited unsuccessfully")),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "linux")]
trait PipeRead: Read + std::os::fd::AsRawFd {}
#[cfg(target_os = "linux")]
impl<T: Read + std::os::fd::AsRawFd> PipeRead for T {}

#[cfg(target_os = "linux")]
struct RunContext<'a> {
    init: &'a protocol::Init,
    deadline: Instant,
    cancelled: &'a dyn Fn() -> bool,
}

#[cfg(target_os = "linux")]
fn supervise(
    child: &mut Child,
    pipes: (Box<dyn PipeRead>, Box<dyn PipeRead>),
    input: std::process::ChildStdin,
    mut broker: SourceBroker,
    context: RunContext<'_>,
) -> io::Result<ResultState<Value>> {
    let (mut protocol_pipe, mut diagnostic_pipe) = pipes;
    let init = context.init;
    let deadline = context.deadline;
    let mut outgoing = VecDeque::<u8>::from(protocol::encode_host_frame(&HostFrame {
        version: protocol::VERSION,
        message: HostMessage::Init(init.clone()),
    })?);
    let mut protocol_decoder = FrameDecoder::default();
    let mut worker_bytes = 0usize;
    let mut diagnostic_bytes = 0usize;
    let mut diagnostic_tail = Vec::new();
    let mut protocol_eof = false;
    let mut diagnostic_eof = false;
    let mut input = Some(input);
    let mut terminal: Option<ResultState<Value>> = None;
    let mut exit_status: Option<ExitStatus> = None;

    loop {
        if (context.cancelled)() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "extension request cancelled",
            ));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "extension request deadline exceeded",
            ));
        }
        let input_fd = input.as_ref().map_or(-1, std::os::fd::AsRawFd::as_raw_fd);
        let mut descriptors = [
            libc::pollfd {
                fd: input_fd,
                events: if outgoing.is_empty() {
                    0
                } else {
                    libc::POLLOUT
                },
                revents: 0,
            },
            libc::pollfd {
                fd: if protocol_eof {
                    -1
                } else {
                    protocol_pipe.as_raw_fd()
                },
                events: if protocol_eof { 0 } else { libc::POLLIN },
                revents: 0,
            },
            libc::pollfd {
                fd: if diagnostic_eof {
                    -1
                } else {
                    diagnostic_pipe.as_raw_fd()
                },
                events: if diagnostic_eof { 0 } else { libc::POLLIN },
                revents: 0,
            },
        ];
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout = POLL_INTERVAL.min(remaining).as_millis().max(1) as i32;
        // SAFETY: descriptors points to three initialized pollfd records for open child pipes.
        let polled =
            unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, timeout) };
        if polled < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
        if descriptors[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0
            && !outgoing.is_empty()
        {
            return Err(invalid("extension helper closed its input early"));
        }
        if descriptors[0].revents & libc::POLLOUT != 0 {
            write_pending(
                input.as_mut().expect("pollable input exists"),
                &mut outgoing,
            )?;
        }
        if descriptors[1].revents & (libc::POLLIN | libc::POLLHUP) != 0 && !protocol_eof {
            let frames = drain_protocol(
                &mut protocol_pipe,
                &mut protocol_decoder,
                &mut worker_bytes,
                &mut protocol_eof,
            )?;
            for frame in frames {
                if terminal.is_some() {
                    return Err(invalid("worker sent a message after its terminal result"));
                }
                match frame.message {
                    WorkerMessage::Read(request) => {
                        let response = match broker.read(request) {
                            Ok(response) => response,
                            Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
                                return Err(error);
                            }
                            Err(error) => ReadResult {
                                read_id: request.read_id,
                                bytes: Vec::new(),
                                eof: false,
                                error: Some(bounded_reason(&error.to_string())),
                            },
                        };
                        append_host_frame(&mut outgoing, HostMessage::ReadResult(response))?;
                    }
                    WorkerMessage::Result(result) => {
                        terminal = Some(protocol::validate_result(init, &result)?);
                    }
                }
            }
        }
        if descriptors[2].revents & (libc::POLLIN | libc::POLLHUP) != 0 && !diagnostic_eof {
            drain_diagnostics(
                &mut diagnostic_pipe,
                &mut diagnostic_bytes,
                &mut diagnostic_tail,
                &mut diagnostic_eof,
            )?;
        }
        if terminal.is_some() && outgoing.is_empty() {
            input.take();
        }
        if let Some(status) = child.try_wait()? {
            exit_status = Some(status);
        }
        if let Some(status) = exit_status
            && protocol_eof
            && diagnostic_eof
        {
            if !status.success() {
                return Err(invalid("extension helper exited unsuccessfully"));
            }
            return terminal.ok_or_else(|| invalid("extension helper exited without a result"));
        }
    }
}

#[cfg(target_os = "linux")]
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::REQUEST_DEADLINE;
    use crate::data_providers::{Completeness, ContributionId, ResultState, Unit, Value};
    use crate::extensions::manifest::{self, PackageId};
    use crate::extensions::supervisor::{ColumnWork, Supervisor};
    use crate::extensions::{Package, protocol};
    use crate::location::Location;
    use std::io::{self, Write};
    use std::process::Command;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    const CONTRIBUTION: &str = "extension.sensor.reader.distance-cell";
    const INVALID_CONTRIBUTION: &str = "extension.sensor.reader.invalid-distance-cell";
    const FIELD: &str = "extension.sensor.reader.distance";
    const MANIFEST: &str = r#"
api_version = 1
id = "sensor"
entry = "main.lua"

[[providers]]
id = "extension.sensor.reader"

[[providers.fields]]
id = "extension.sensor.reader.distance"
kind = "quantity"
unit = "metres"

[[providers.columns]]
id = "extension.sensor.reader.distance-cell"
label = "Distance"
column_id = "extension.sensor.reader.distance"
field = "extension.sensor.reader.distance"
max_bytes = 1024
max_items = 10

[[providers.columns]]
id = "extension.sensor.reader.invalid-distance-cell"
label = "Invalid distance"
column_id = "extension.sensor.reader.invalid-distance"
field = "extension.sensor.reader.distance"
max_bytes = 1024
max_items = 10
"#;

    fn work(root: &std::path::Path, source_name: &str, script: String) -> ColumnWork {
        let manifest = manifest::parse(
            &PackageId::new("sensor").expect("fixture package ID"),
            MANIFEST,
        )
        .expect("fixture manifest is valid");
        let source = root.join(source_name);
        std::fs::write(&source, b"data").expect("write fixture source");
        ColumnWork {
            package: Arc::new(Package {
                manifest,
                source: script,
            }),
            contribution: ContributionId::new(CONTRIBUTION).expect("fixture contribution ID"),
            source: Location::local(source).expect("fixture source is local"),
            name: "fixture.bin".into(),
            is_dir: false,
            generation_cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    fn normal_script() -> String {
        format!(
            r#"return {{
  ["{CONTRIBUTION}"] = {{
    read = function(ctx)
      local bytes, eof = fyle.read(ctx.source, 0, 4)
      local completeness = {{ kind = "complete" }}
      if not eof then completeness = {{ kind = "truncated", reason = "read bound" }} end
      return {{ state = "ready", field = "{FIELD}",
        value = {{ kind = "quantity", value = #bytes, unit = "metres" }},
        completeness = completeness }}
    end
  }}
}}"#
        )
    }

    fn launcher(mode: &str) -> Arc<dyn Fn() -> io::Result<Command> + Send + Sync> {
        let mode = mode.to_owned();
        Arc::new(move || {
            let mut command = Command::new(std::env::current_exe()?);
            command
                .args([
                    "--exact",
                    "extensions::supervisor::process::tests::worker_subprocess_fixture",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("FYLE_EXTENSION_WORKER_FIXTURE", &mode);
            Ok(command)
        })
    }

    fn fixture_supervisor(mode: &str) -> Supervisor {
        Supervisor::with_launcher(launcher(mode), true).expect("fixture supervisor starts")
    }

    fn ready_pid(path: &std::path::Path) -> u32 {
        let started = Instant::now();
        loop {
            if let Ok(pid) = std::fs::read_to_string(path)
                && let Ok(pid) = pid.parse()
            {
                return pid;
            }
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "helper ready file never contained a PID"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn assert_child_reaped(pid: u32) {
        let mut status = 0;
        // SAFETY: pid came from our direct child fixture; status points to writable storage.
        let result = unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) };
        assert_eq!(result, -1, "helper {pid} should already have been reaped");
        // SAFETY: errno access is valid immediately after waitpid on Linux.
        let errno = unsafe { *libc::__errno_location() };
        assert_eq!(errno, libc::ECHILD, "waitpid should report no child {pid}");
    }

    fn wait_until_child_gone(pid: u32) {
        let started = Instant::now();
        loop {
            // SAFETY: signal zero only checks existence; pid came from the fixture.
            let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
            // SAFETY: errno access is valid immediately after kill on Linux.
            let errno = unsafe { *libc::__errno_location() };
            if result == -1 && errno == libc::ESRCH {
                return;
            }
            assert!(
                started.elapsed() < REQUEST_DEADLINE + Duration::from_secs(2),
                "helper {pid} remained alive"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn worker_subprocess_fixture() {
        let Ok(mode) = std::env::var("FYLE_EXTENSION_WORKER_FIXTURE") else {
            return;
        };
        match mode.as_str() {
            "never_read_init" => {
                if let Some(path) = std::env::var_os("FYLE_EXTENSION_WORKER_READY_FILE") {
                    std::fs::write(path, std::process::id().to_string())
                        .expect("write helper PID fixture");
                }
                std::thread::sleep(Duration::from_secs(60));
            }
            "crash" => {
                let stdin = io::stdin();
                let mut input = stdin.lock();
                let _ = protocol::read_host_frame(&mut input);
                std::process::exit(19);
            }
            "partial_frame" => {
                let stdin = io::stdin();
                let mut input = stdin.lock();
                let _ = protocol::read_host_frame(&mut input);
                let stderr = io::stderr();
                let mut output = stderr.lock();
                output.write_all(&[0, 0]).unwrap();
                output.flush().unwrap();
            }
            "oversized_frame" => {
                let stdin = io::stdin();
                let mut input = stdin.lock();
                let _ = protocol::read_host_frame(&mut input);
                let stderr = io::stderr();
                let mut output = stderr.lock();
                output
                    .write_all(&((protocol::MAX_FRAME_BYTES as u32 + 1).to_be_bytes()))
                    .unwrap();
                output.flush().unwrap();
            }
            "duplicate_terminal" => {
                let stdin = io::stdin();
                let mut input = stdin.lock();
                let _ = protocol::read_host_frame(&mut input);
                let stderr = io::stderr();
                let mut output = stderr.lock();
                let frame = protocol::WorkerFrame {
                    version: protocol::VERSION,
                    message: protocol::WorkerMessage::Result(protocol::ContributionResult {
                        field: None,
                        state: protocol::OutputState::Absent,
                    }),
                };
                protocol::write_worker_frame(&mut output, &frame).unwrap();
                protocol::write_worker_frame(&mut output, &frame).unwrap();
                output.flush().unwrap();
            }
            "terminal_then_hang" => {
                let stdin = io::stdin();
                let mut input = stdin.lock();
                let _ = protocol::read_host_frame(&mut input);
                let stderr = io::stderr();
                let mut output = stderr.lock();
                protocol::write_worker_frame(
                    &mut output,
                    &protocol::WorkerFrame {
                        version: protocol::VERSION,
                        message: protocol::WorkerMessage::Result(protocol::ContributionResult {
                            field: None,
                            state: protocol::OutputState::Absent,
                        }),
                    },
                )
                .unwrap();
                output.flush().unwrap();
                std::thread::sleep(Duration::from_secs(60));
            }
            "diagnostics_overflow" => {
                let stdout = io::stdout();
                let mut output = stdout.lock();
                output
                    .write_all(&vec![b'x'; super::STDERR_LIMIT + 1])
                    .unwrap();
                output.flush().unwrap();
                std::thread::sleep(Duration::from_secs(60));
            }
            "worker" => {
                let stdin = io::stdin();
                let stderr = io::stderr();
                let mut input = stdin.lock();
                let mut output = stderr.lock();
                crate::extensions::worker::run(&mut input, &mut output).unwrap();
            }
            _ => panic!("unknown subprocess fixture mode"),
        }
    }

    #[test]
    fn actual_supervisor_runs_worker_and_brokers_native_source_bytes() {
        let temp = TempDir::new().unwrap();
        let supervisor = fixture_supervisor("worker");
        let ticket = supervisor
            .submit(work(temp.path(), "source", normal_script()))
            .unwrap();
        assert_eq!(
            ticket.wait().unwrap(),
            ResultState::Ready {
                value: Value::quantity(4.0, Unit::Metres).unwrap(),
                completeness: Completeness::Complete,
            }
        );
        supervisor.shutdown().unwrap();
    }

    #[test]
    fn enabled_package_flows_through_catalog_metadata_cache_and_partial_cell_projection() {
        use crate::columns::catalog::Availability;
        use crate::columns::{ColumnId, ProviderColumnRequest};
        use crate::data_providers::ResultState;
        use crate::extensions::catalog::install_packages;
        use crate::extensions::load_enabled;
        use crate::links::LinkState;
        use crate::panel::metadata::model::{CapturedRow, Field, MetadataQueue};
        use std::sync::atomic::AtomicBool;

        let temp = TempDir::new().unwrap();
        let extensions_root = temp.path().join("extensions");
        let package_root = extensions_root.join("sensor");
        std::fs::create_dir_all(&package_root).unwrap();
        std::fs::write(package_root.join("manifest.toml"), MANIFEST).unwrap();
        let script = format!(
            r#"return {{
                ["{CONTRIBUTION}"] = {{ read=function()
                    return {{state="ready", field="{FIELD}",
                        value={{kind="quantity", value=12.36, unit="metres"}},
                        completeness={{kind="truncated", reason="sample limit"}}}}
                end }},
                ["{INVALID_CONTRIBUTION}"] = {{ read=function()
                    return {{state="ready", field="{FIELD}",
                        value={{kind="quantity", value=12.36, unit="seconds"}},
                        completeness={{kind="complete"}}}}
                end }}
            }}"#
        );
        std::fs::write(package_root.join("main.lua"), &script).unwrap();
        let enabled = [PackageId::new("sensor").unwrap()];
        let report = load_enabled(&extensions_root, &enabled);
        assert!(report.diagnostics.is_empty());
        assert_eq!(report.packages.len(), 1);

        let starts = Arc::new(AtomicUsize::new(0));
        let launcher_starts = starts.clone();
        let supervisor = Supervisor::with_launcher(
            Arc::new(move || {
                launcher_starts.fetch_add(1, Ordering::SeqCst);
                launcher("worker")()
            }),
            true,
        )
        .unwrap();
        let selected = [
            ColumnId::new("extension.sensor.reader.distance").unwrap(),
            ColumnId::new("extension.sensor.reader.invalid-distance").unwrap(),
        ];
        let available =
            install_packages(report.packages, Some(supervisor.clone()), &extensions_root);
        assert_eq!(
            starts.load(Ordering::SeqCst),
            0,
            "catalog build must not execute Lua"
        );
        assert!(selected.iter().all(|id| matches!(
            available.catalog.descriptor(id).map(|descriptor| &descriptor.availability),
            Some(Availability::Available)
        )));

        let source = temp.path().join("track.dat");
        std::fs::write(&source, b"fixture").unwrap();
        let location = Location::local(source.clone()).unwrap();
        let directory_request = ProviderColumnRequest {
            location: location.clone(),
            name: "track.dat".into(),
            is_dir: true,
            demand: available.catalog.provider_demand(&selected[0]).unwrap(),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        assert!(matches!(
            available
                .catalog
                .read_provider_cell(&selected[0], &directory_request),
            ResultState::Unavailable(_)
        ));
        assert_eq!(
            starts.load(Ordering::SeqCst),
            0,
            "directories never launch the worker"
        );

        let mut queue = MetadataQueue::default();
        let fields = selected
            .iter()
            .cloned()
            .map(Field::Provider)
            .collect::<Vec<_>>();
        queue.request_row(
            &location,
            1,
            &fields,
            None,
            LinkState::NotLink,
            Some(CapturedRow {
                name: "track.dat".into(),
                is_dir: false,
            }),
        );
        let request = queue.take_next().unwrap();
        let values = crate::panel::metadata::reader::read_metadata(&request, &available.catalog);
        assert!(queue.complete(request, values));
        let cells = queue.get(&source, 1).unwrap();
        let partial = cells.provider_cell(&selected[0]).unwrap();
        assert!(matches!(
            partial,
            ResultState::Ready {
                completeness: Completeness::Truncated(reason), ..
            } if reason == "sample limit"
        ));
        assert_eq!(crate::columns::cell::project(partial).text, "… 12.4 m");
        assert!(matches!(
            cells.provider_cell(&selected[1]),
            Some(ResultState::Failed(_))
        ));
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        queue.request_row(
            &location,
            1,
            &fields,
            None,
            LinkState::NotLink,
            Some(CapturedRow {
                name: "track.dat".into(),
                is_dir: false,
            }),
        );
        assert!(
            queue.take_next().is_none(),
            "completed states suppress retries"
        );

        let mut stale_queue = MetadataQueue::default();
        stale_queue.request_row(
            &location,
            5,
            &fields,
            None,
            LinkState::NotLink,
            Some(CapturedRow {
                name: "track.dat".into(),
                is_dir: false,
            }),
        );
        let stale = stale_queue.take_next().unwrap();
        stale_queue.sync_generation(6);
        let stale_values =
            crate::panel::metadata::reader::read_metadata(&stale, &available.catalog);
        assert!(!stale_queue.complete(stale, stale_values));
        assert!(stale_queue.get(&source, 6).is_none());
        assert_eq!(
            starts.load(Ordering::SeqCst),
            2,
            "cancelled stale work skips child launch"
        );

        let unavailable = crate::columns::Catalog::with_defaults();
        assert!(matches!(
            unavailable.display_descriptor(&selected[0]).availability,
            Availability::Unavailable(_)
        ));
        let again = load_enabled(&extensions_root, &enabled);
        let restored = install_packages(again.packages, Some(supervisor.clone()), &extensions_root);
        assert_eq!(
            selected.len(),
            2,
            "saved selection is independent of availability"
        );
        assert!(selected.iter().all(|id| matches!(
            restored.catalog.descriptor(id).map(|descriptor| &descriptor.availability),
            Some(Availability::Available)
        )));
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        supervisor.shutdown().unwrap();
    }

    #[test]
    fn helper_frames_fail_closed_for_crash_partial_oversize_duplicate_and_post_terminal_hang() {
        for (mode, expected) in [
            ("crash", "unsuccessfully"),
            ("partial_frame", "partial protocol frame"),
            ("oversized_frame", "exceeds the limit"),
            ("duplicate_terminal", "after its terminal result"),
            ("terminal_then_hang", "deadline exceeded"),
            ("diagnostics_overflow", "diagnostics exceeded"),
        ] {
            let temp = TempDir::new().unwrap();
            let supervisor = fixture_supervisor(mode);
            let ticket = supervisor
                .submit(work(temp.path(), "source", normal_script()))
                .unwrap();
            let state = ticket.wait().unwrap();
            let ResultState::Failed(reason) = state else {
                panic!("mode={mode} unexpectedly returned {state:?}");
            };
            assert!(reason.contains(expected), "mode={mode}: {reason}");
            supervisor.shutdown().unwrap();
        }
    }

    #[test]
    fn large_init_never_reading_is_deadlined_and_reaped() {
        let temp = TempDir::new().unwrap();
        let supervisor = fixture_supervisor("never_read_init");
        let script = format!("--{}\n{}", "x".repeat(manifest::SCRIPT_BYTES - 4), " ");
        assert_eq!(script.len(), manifest::SCRIPT_BYTES);
        let started = Instant::now();
        let ticket = supervisor
            .submit(work(temp.path(), "source", script))
            .unwrap();
        assert!(matches!(ticket.wait().unwrap(), ResultState::Failed(_)));
        assert!(started.elapsed() < REQUEST_DEADLINE + Duration::from_secs(2));
        supervisor.shutdown().unwrap();
    }

    #[test]
    fn active_cancellation_reaps_before_the_next_request_starts() {
        let temp = TempDir::new().unwrap();
        let starts = Arc::new(AtomicUsize::new(0));
        let launcher_starts = starts.clone();
        let ready_path = temp.path().join("first-helper-ready");
        let launcher_ready = ready_path.clone();
        let supervisor = Supervisor::with_launcher(
            Arc::new(move || {
                let index = launcher_starts.fetch_add(1, Ordering::SeqCst);
                let mode = if index == 0 {
                    "never_read_init"
                } else {
                    "worker"
                };
                let mut command = launcher(mode)()?;
                if index == 0 {
                    command.env("FYLE_EXTENSION_WORKER_READY_FILE", &launcher_ready);
                }
                Ok(command)
            }),
            true,
        )
        .unwrap();
        let first = supervisor
            .submit(work(temp.path(), "first", normal_script()))
            .unwrap();
        let started = Instant::now();
        while !ready_path.exists() && started.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(ready_path.exists(), "first child did not start");
        let first_pid = ready_pid(&ready_path);
        first.cancel();
        let second = supervisor
            .submit(work(temp.path(), "second", normal_script()))
            .unwrap();
        assert!(matches!(first.wait().unwrap(), ResultState::Failed(_)));
        assert!(matches!(second.wait().unwrap(), ResultState::Ready { .. }));
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        assert_child_reaped(first_pid);
        supervisor.shutdown().unwrap();
    }

    #[test]
    fn explicit_shutdown_kills_and_reaps_active_helper_before_returning() {
        let temp = TempDir::new().unwrap();
        let ready_path = temp.path().join("helper-ready");
        let launcher_ready = ready_path.clone();
        let supervisor = Supervisor::with_launcher(
            Arc::new(move || {
                let mut command = launcher("never_read_init")()?;
                command.env("FYLE_EXTENSION_WORKER_READY_FILE", &launcher_ready);
                Ok(command)
            }),
            true,
        )
        .unwrap();
        let ticket = supervisor
            .submit(work(temp.path(), "source", normal_script()))
            .unwrap();
        let started = Instant::now();
        while !ready_path.exists() && started.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(ready_path.exists(), "helper did not start");
        let pid = ready_pid(&ready_path);
        let shutdown_started = Instant::now();
        supervisor.shutdown().unwrap();
        assert!(shutdown_started.elapsed() < Duration::from_secs(1));
        assert!(matches!(ticket.wait().unwrap(), ResultState::Failed(_)));
        assert_child_reaped(pid);
    }

    #[test]
    fn dropping_last_supervisor_handle_kills_and_reaps_active_helper() {
        let temp = TempDir::new().unwrap();
        let ready_path = temp.path().join("drop-helper-ready");
        let launcher_ready = ready_path.clone();
        let supervisor = Supervisor::with_launcher(
            Arc::new(move || {
                let mut command = launcher("never_read_init")()?;
                command.env("FYLE_EXTENSION_WORKER_READY_FILE", &launcher_ready);
                Ok(command)
            }),
            true,
        )
        .unwrap();
        let ticket = supervisor
            .submit(work(temp.path(), "source", normal_script()))
            .unwrap();
        let started = Instant::now();
        while !ready_path.exists() && started.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(ready_path.exists(), "helper did not start");
        let pid = ready_pid(&ready_path);
        drop(supervisor);
        wait_until_child_gone(pid);
        assert_child_reaped(pid);
        assert!(matches!(ticket.wait().unwrap(), ResultState::Failed(_)));
    }

    #[test]
    fn lua_instruction_hook_limits_top_level_loops() {
        let temp = TempDir::new().unwrap();
        let supervisor = fixture_supervisor("worker");
        let script = format!("while true do end\nreturn {{}} -- {CONTRIBUTION}");
        let ticket = supervisor
            .submit(work(temp.path(), "source", script))
            .unwrap();
        let ResultState::Failed(reason) = ticket.wait().unwrap() else {
            panic!("top-level loop should fail at the instruction limit");
        };
        assert!(reason.contains("instruction limit"), "{reason}");
        supervisor.shutdown().unwrap();
    }

    #[test]
    fn lua_instruction_hook_covers_callback_execution() {
        let temp = TempDir::new().unwrap();
        let supervisor = fixture_supervisor("worker");
        let script = format!(
            r#"return {{ ["{CONTRIBUTION}"] = {{ read = function(ctx) while true do end end }} }}"#
        );
        let ticket = supervisor
            .submit(work(temp.path(), "source", script))
            .unwrap();
        let ResultState::Failed(reason) = ticket.wait().unwrap() else {
            panic!("callback loop should fail at the instruction limit");
        };
        assert!(reason.contains("instruction limit"), "{reason}");
        supervisor.shutdown().unwrap();
    }
}
