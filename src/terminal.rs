//! "Open terminal here": which terminal emulator to start, and how to tell
//! it the working directory.

use std::path::Path;

/// Known terminals, in order of preference, with the arguments that open
/// a window in a directory (`{dir}`). The process also starts in that
/// directory, which is all the others need.
const KNOWN: &[(&str, &[&str])] = &[
    ("x-terminal-emulator", &[]),
    ("ptyxis", &["--new-window", "--working-directory={dir}"]),
    ("gnome-terminal", &["--working-directory={dir}"]),
    ("konsole", &["--workdir", "{dir}"]),
    ("xfce4-terminal", &["--working-directory={dir}"]),
    ("kitty", &["--directory", "{dir}"]),
    ("alacritty", &["--working-directory", "{dir}"]),
    ("wezterm", &["start", "--cwd", "{dir}"]),
    ("foot", &["--working-directory={dir}"]),
    ("ghostty", &["--working-directory={dir}"]),
    ("xterm", &[]),
];

/// The command line opening a terminal in `dir`: the configured command if
/// any, else `$TERMINAL`, else the first known terminal that `exists`.
/// `{dir}` in configured arguments is replaced by the directory.
pub fn command(
    configured: &[String],
    env_terminal: Option<&str>,
    dir: &Path,
    exists: impl Fn(&str) -> bool,
) -> Option<Vec<String>> {
    let dir = dir.to_string_lossy();
    let expand =
        |args: &[&str]| -> Vec<String> { args.iter().map(|a| a.replace("{dir}", &dir)).collect() };
    if !configured.is_empty() {
        let args: Vec<&str> = configured.iter().map(String::as_str).collect();
        return Some(expand(&args));
    }
    let known = |name: &str| KNOWN.iter().find(|(n, _)| *n == name);
    if let Some(name) = env_terminal.filter(|t| !t.is_empty()) {
        let mut cmd = vec![name.to_string()];
        let program = Path::new(name).file_name().and_then(|n| n.to_str());
        if let Some((_, args)) = program.and_then(known) {
            cmd.extend(expand(args));
        }
        return Some(cmd);
    }
    KNOWN
        .iter()
        .find(|(name, _)| exists(name))
        .map(|(name, args)| {
            std::iter::once(name.to_string())
                .chain(expand(args))
                .collect()
        })
}

/// What precedes the program to run in a terminal: `-e` for most.
fn run_separator(terminal: &str) -> Option<&'static str> {
    let name = Path::new(terminal).file_name()?.to_str()?;
    match name {
        "gnome-terminal" | "ptyxis" | "wezterm" => Some("--"),
        // These take the command as the rest of their arguments.
        "kitty" | "foot" => None,
        _ => Some("-e"),
    }
}

/// The command line running `argv` in a terminal window started in `dir`
/// (same choice of terminal as [`command`]).
pub fn run_command(
    configured: &[String],
    env_terminal: Option<&str>,
    dir: &Path,
    exists: impl Fn(&str) -> bool,
    argv: &[String],
) -> Option<Vec<String>> {
    let mut command = command(configured, env_terminal, dir, exists)?;
    if let Some(separator) = run_separator(command.first()?) {
        command.push(separator.into());
    }
    command.extend(argv.iter().cloned());
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn configured_command_wins_and_expands_dir() {
        let cmd = command(
            &strings(&["kitty", "-d", "{dir}"]),
            Some("xterm"),
            Path::new("/tmp/x"),
            |_| true,
        );
        assert_eq!(cmd, Some(strings(&["kitty", "-d", "/tmp/x"])));
    }

    #[test]
    fn env_terminal_gets_known_arguments() {
        let dir = Path::new("/w");
        let cmd = command(&[], Some("/usr/bin/alacritty"), dir, |_| false);
        assert_eq!(
            cmd,
            Some(strings(&[
                "/usr/bin/alacritty",
                "--working-directory",
                "/w"
            ]))
        );
        let cmd = command(&[], Some("st"), dir, |_| false);
        assert_eq!(cmd, Some(strings(&["st"])));
    }

    #[test]
    fn falls_back_to_first_installed_terminal() {
        let dir = Path::new("/w");
        let cmd = command(&[], None, dir, |name| name == "konsole" || name == "xterm");
        assert_eq!(cmd, Some(strings(&["konsole", "--workdir", "/w"])));
        let cmd = command(&[], Some(""), dir, |name| name == "xterm");
        assert_eq!(cmd, Some(strings(&["xterm"])));
        assert_eq!(command(&[], None, dir, |_| false), None);
    }

    #[test]
    fn runs_a_program_in_a_terminal() {
        let argv = strings(&["vim", "/a b"]);
        let run = |term: Option<&str>| run_command(&[], term, Path::new("/w"), |_| false, &argv);
        assert_eq!(
            run(Some("konsole")),
            Some(strings(&[
                "konsole",
                "--workdir",
                "/w",
                "-e",
                "vim",
                "/a b"
            ]))
        );
        assert_eq!(
            run(Some("gnome-terminal")),
            Some(strings(&[
                "gnome-terminal",
                "--working-directory=/w",
                "--",
                "vim",
                "/a b"
            ]))
        );
        assert_eq!(
            run(Some("/usr/bin/kitty")),
            Some(strings(&[
                "/usr/bin/kitty",
                "--directory",
                "/w",
                "vim",
                "/a b"
            ]))
        );
        assert_eq!(run(None), None);
    }
}
