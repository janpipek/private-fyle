//! Desktop entries (`.desktop` files): parsing, desktop file IDs and the
//! `Exec` line with its field codes, per the freedesktop Desktop Entry
//! spec; UI-independent.

use std::path::{Path, PathBuf};

use crate::ini::Ini;

/// The parts of an application entry that Open With needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    /// Desktop file ID: `kde4-konsole.desktop` for `kde4/konsole.desktop`.
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    /// The `Exec` value with INI escapes resolved, still holding field codes.
    pub exec: String,
    pub terminal: bool,
    pub hidden: bool,
    pub no_display: bool,
    pub try_exec: Option<String>,
    pub mime_types: Vec<String>,
}

/// The ID of `file` inside the `applications` directory `root`.
pub fn desktop_id(root: &Path, file: &Path) -> Option<String> {
    let relative = file.strip_prefix(root).ok()?.to_str()?;
    let stem = relative.strip_suffix(".desktop")?;
    (!stem.is_empty()).then(|| format!("{}.desktop", stem.replace('/', "-")))
}

/// Parses an entry; `None` unless it is a `Type=Application` with a name
/// and an `Exec`. `locales` are keys like `de_DE`, `de` for `Name[de]`.
pub fn parse(id: &str, path: &Path, text: &str, locales: &[String]) -> Option<DesktopEntry> {
    let ini = Ini::parse(text);
    let get = |key: &str| ini.get("Desktop Entry", key);
    if get("Type") != Some("Application") {
        return None;
    }
    let name = locales
        .iter()
        .find_map(|l| get(&format!("Name[{l}]")))
        .or_else(|| get("Name"))
        .map(unescape)
        .filter(|n| !n.is_empty())?;
    let exec = unescape(get("Exec").filter(|e| !e.is_empty())?);
    let flag = |key: &str| get(key) == Some("true");
    Some(DesktopEntry {
        id: id.to_string(),
        path: path.to_path_buf(),
        name,
        exec,
        terminal: flag("Terminal"),
        hidden: flag("Hidden"),
        no_display: flag("NoDisplay"),
        try_exec: get("TryExec").filter(|t| !t.is_empty()).map(unescape),
        mime_types: get("MimeType")
            .map(|v| {
                v.split(';')
                    .filter(|m| !m.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Resolves `\s \n \t \r \\` in a string value.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Splits an `Exec` value into arguments: double quotes group, and inside
/// them a backslash escapes `"`, `` ` ``, `$` and `\`.
pub fn split_exec(exec: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => match chars.next() {
                Some(e @ ('"' | '`' | '$' | '\\')) => current.push(e),
                Some(other) => {
                    current.push('\\');
                    current.push(other);
                }
                None => current.push('\\'),
            },
            c if c.is_whitespace() && !quoted => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if quoted {
        return Err("unterminated quote in Exec".into());
    }
    if started {
        args.push(current);
    }
    Ok(args)
}

/// The program of an `Exec` line (its first argument).
pub fn exec_program(exec: &str) -> Option<String> {
    split_exec(exec).ok()?.into_iter().next()
}

/// The command line for opening `files`: field codes expanded. `%f`/`%u`
/// take the first file, `%F`/`%U` all of them (`%u`/`%U` as `file://`
/// URIs); `%i`, `%c`, `%k` and the deprecated codes are dropped, `%%` is a
/// percent sign. An `Exec` without file codes gets no file, as specified.
pub fn expand_exec(exec: &str, files: &[&Path]) -> Result<Vec<String>, String> {
    let mut command = Vec::new();
    for arg in split_exec(exec)? {
        match arg.as_str() {
            "%F" => command.extend(files.iter().map(|f| f.display().to_string())),
            "%U" => command.extend(files.iter().map(|f| file_uri(f))),
            _ => {
                let expanded = expand_arg(&arg, files);
                if !expanded.is_empty() || arg.is_empty() {
                    command.push(expanded);
                }
            }
        }
    }
    if command.is_empty() {
        return Err("empty Exec".into());
    }
    Ok(command)
}

fn expand_arg(arg: &str, files: &[&Path]) -> String {
    let mut out = String::new();
    let mut chars = arg.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('f' | 'F') => out.extend(files.first().map(|f| f.display().to_string())),
            Some('u' | 'U') => out.extend(files.first().map(|f| file_uri(f))),
            // %i %c %k, deprecated codes and anything unknown.
            _ => {}
        }
    }
    out
}

/// `file://` URI of an absolute path, percent-encoding all but unreserved
/// characters and `/`.
pub fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for &b in path.to_string_lossy().as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> &Path {
        Path::new(s)
    }

    fn expand(exec: &str, files: &[&str]) -> Vec<String> {
        let files: Vec<&Path> = files.iter().map(|f| p(f)).collect();
        expand_exec(exec, &files).unwrap()
    }

    #[test]
    fn ids_flatten_subdirectories() {
        let root = p("/usr/share/applications");
        let id = |f: &str| desktop_id(root, p(f));
        assert_eq!(
            id("/usr/share/applications/a.desktop"),
            Some("a.desktop".into())
        );
        assert_eq!(
            id("/usr/share/applications/kde4/konsole.desktop"),
            Some("kde4-konsole.desktop".into())
        );
        assert_eq!(id("/usr/share/applications/readme.txt"), None);
        assert_eq!(id("/elsewhere/a.desktop"), None);
    }

    #[test]
    fn parses_entries() {
        let text = "[Desktop Entry]\nType=Application\nName=Edit\nName[de]=Bearbeiten\n\
                    Exec=edit %F\nTerminal=true\nNoDisplay=true\nTryExec=edit\n\
                    MimeType=text/plain;text/x-rust;\n[Desktop Action x]\nName=Other\n";
        let none: [String; 0] = [];
        let e = parse("e.desktop", p("/a/e.desktop"), text, &none).unwrap();
        assert_eq!(e.name, "Edit");
        assert!(e.terminal && e.no_display && !e.hidden);
        assert_eq!(e.try_exec.as_deref(), Some("edit"));
        assert_eq!(e.mime_types, ["text/plain", "text/x-rust"]);
        let de = parse("e.desktop", p("/a"), text, &["de_DE".into(), "de".into()]).unwrap();
        assert_eq!(de.name, "Bearbeiten");
        assert!(
            parse(
                "x",
                p("/a"),
                "[Desktop Entry]\nType=Link\nName=x\nExec=x",
                &none
            )
            .is_none()
        );
        assert!(
            parse(
                "x",
                p("/a"),
                "[Desktop Entry]\nType=Application\nName=x",
                &none
            )
            .is_none()
        );
        assert!(parse("x", p("/a"), "Name=x\nExec=x", &none).is_none());
    }

    #[test]
    fn splits_quotes_and_escapes() {
        assert_eq!(
            split_exec(r#"prog "a b" c\d "q\"x\$y\\z" "" end"#).unwrap(),
            ["prog", "a b", r"c\d", r#"q"x$y\z"#, "", "end"]
        );
        assert!(split_exec(r#"prog "open"#).is_err());
        assert_eq!(exec_program("  env A=1 prog").as_deref(), Some("env"));
    }

    #[test]
    fn exec_escapes_are_resolved_before_splitting() {
        // `\\\\` in the file is one backslash in the string value, which
        // then escapes the next character inside quotes.
        let text = "[Desktop Entry]\nType=Application\nName=n\nExec=prog \"a\\\\\\\\b\\sc\" %f\n";
        let none: [String; 0] = [];
        let e = parse("i", p("/i"), text, &none).unwrap();
        assert_eq!(
            expand_exec(&e.exec, &[p("/f")]).unwrap(),
            ["prog", "a\\b c", "/f"]
        );
    }

    #[test]
    fn expands_file_codes() {
        assert_eq!(expand("ed %f", &["/a b/x.txt", "/y"]), ["ed", "/a b/x.txt"]);
        assert_eq!(expand("ed %F", &["/x", "/y z"]), ["ed", "/x", "/y z"]);
        assert_eq!(expand("ed %u", &["/a b/é"]), ["ed", "file:///a%20b/%C3%A9"]);
        assert_eq!(
            expand("ed %U", &["/x", "/y"]),
            ["ed", "file:///x", "file:///y"]
        );
        assert_eq!(expand("ed --file=%f", &["/x"]), ["ed", "--file=/x"]);
        assert_eq!(expand("ed 100%% %f", &["/x"]), ["ed", "100%", "/x"]);
    }

    #[test]
    fn drops_unwanted_codes() {
        assert_eq!(
            expand("ed %i %c %k %d %D %n %N %v %m %f", &["/x"]),
            ["ed", "/x"]
        );
        assert_eq!(expand("ed \"\" %f", &["/x"]), ["ed", "", "/x"]);
        assert_eq!(expand("ed -x", &["/x"]), ["ed", "-x"]);
        assert_eq!(expand("ed %F", &[]), ["ed"]);
        assert!(expand_exec("%i", &[p("/x")]).is_err());
    }
}
