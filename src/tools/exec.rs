//! Running planned actions. Blocking; the UI calls this off the main thread.
//! [`Action::Pick`] is resolved by the UI first (it asks the user).

use std::process::{Command, Stdio};

use super::{Action, CommandSpec, Output};

/// Result of a finished action.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// One line for the status bar.
    pub summary: String,
    /// Combined stdout/stderr, for [`Output::Log`] commands.
    pub log: String,
    /// Whether the panels should reload.
    pub refresh: bool,
}

pub fn run(action: Action) -> Result<Report, String> {
    match action {
        Action::OpenUrl(url) => {
            open::that_detached(&url).map_err(|e| format!("{url}: {e}"))?;
            Ok(Report {
                summary: format!("Opened {url}"),
                ..Report::default()
            })
        }
        Action::Run(spec) => run_command(&spec),
        Action::Sequence(actions) => {
            let mut total = Report::default();
            let count = actions.len();
            for action in actions {
                let report = run(action)?;
                total.log.push_str(&report.log);
                total.refresh |= report.refresh;
            }
            total.summary = format!("Finished {count} commands");
            Ok(total)
        }
        Action::Pick { .. } => Err("a pick must be resolved by the UI".into()),
        Action::Native(job) => {
            let refresh = job.refresh();
            Ok(Report {
                summary: job.run()?,
                refresh,
                ..Report::default()
            })
        }
    }
}

fn run_command(spec: &CommandSpec) -> Result<Report, String> {
    let name = spec.program.display().to_string();
    let mut command = Command::new(&spec.program);
    command.args(&spec.args).current_dir(&spec.cwd);
    if spec.output == Output::Detached {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{name}: {e}"))?;
        return Ok(Report {
            summary: format!("Started {name}"),
            ..Report::default()
        });
    }
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{name}: {e}"))?;
    let mut log = String::from_utf8_lossy(&output.stdout).into_owned();
    log.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        let last = log
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        return Err(format!("{name} failed ({}): {last}", output.status));
    }
    Ok(Report {
        summary: format!("{name} finished"),
        log,
        refresh: spec.refresh,
    })
}
