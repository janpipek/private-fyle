//! Windows file associations and command templates for Open With.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use winreg::RegKey;
use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER};

use crate::apps::Application;

/// File extension and directory associations read from the registry.
#[derive(Debug, Default)]
pub struct Catalog {
    by_type: HashMap<String, Vec<Application>>,
}

impl Catalog {
    /// Reads Windows associations; call only on a background thread.
    pub fn load() -> Self {
        let classes = RegKey::predef(HKEY_CLASSES_ROOT);
        let user = RegKey::predef(HKEY_CURRENT_USER);
        let mut by_type = HashMap::new();
        for extension in classes
            .enum_keys()
            .flatten()
            .filter(|key| key.starts_with('.'))
        {
            let apps = associations(&classes, &user, &extension);
            if !apps.is_empty() {
                by_type.insert(extension.to_ascii_lowercase(), apps);
            }
        }
        let mut folders = associations(&classes, &user, "Directory");
        folders.extend(associations(&classes, &user, "Folder"));
        folders.extend(directory_shell_apps(&classes));
        let mut seen = HashSet::new();
        folders.retain(|app| seen.insert(app.exec.to_ascii_lowercase()));
        if !folders.is_empty() {
            by_type.insert("<directory>".into(), folders);
        }
        Self { by_type }
    }

    /// Registered applications for a file extension or directory.
    pub fn applications_for(&self, name: &str, is_dir: bool) -> Vec<Application> {
        let key = if is_dir {
            "<directory>".to_string()
        } else {
            Path::new(name)
                .extension()
                .map(|ext| format!(".{}", ext.to_string_lossy().to_ascii_lowercase()))
                .unwrap_or_default()
        };
        self.by_type.get(&key).cloned().unwrap_or_default()
    }
}

fn associations(classes: &RegKey, user: &RegKey, kind: &str) -> Vec<Application> {
    let user_path = format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\{kind}");
    let user_key = user.open_subkey(&user_path).ok();
    let default = user_key
        .as_ref()
        .and_then(|key| key.open_subkey("UserChoice").ok())
        .and_then(|key| key.get_value::<String, _>("ProgId").ok())
        .or_else(|| {
            classes
                .open_subkey(kind)
                .ok()?
                .get_value::<String, _>("")
                .ok()
        });

    let mut ids = Vec::new();
    if let Some(default) = default.as_ref() {
        ids.push(default.clone());
    }
    for key in [classes.open_subkey(kind).ok(), user_key]
        .into_iter()
        .flatten()
    {
        if let Ok(progids) = key.open_subkey("OpenWithProgids") {
            ids.extend(progids.enum_values().flatten().map(|(name, _)| name));
        }
        if let Ok(list) = key.open_subkey("OpenWithList") {
            ids.extend(
                list.enum_keys()
                    .flatten()
                    .map(|exe| format!("Applications\\{exe}")),
            );
            ids.extend(
                list.enum_values()
                    .flatten()
                    .filter_map(|(name, _)| {
                        (name != "MRUList")
                            .then(|| list.get_value::<String, _>(&name).ok())
                            .flatten()
                    })
                    .map(|exe| format!("Applications\\{exe}")),
            );
        }
    }
    if kind == "Directory" {
        ids.push("Folder".into());
    }

    assemble(default.as_deref(), ids, |id| resolve(classes, id))
}

fn assemble(
    default: Option<&str>,
    ids: Vec<String>,
    resolve: impl Fn(&str) -> Option<Application>,
) -> Vec<Application> {
    let mut seen_ids = HashSet::new();
    let mut seen_commands = HashSet::new();
    let mut apps = Vec::new();
    for id in ids {
        if !seen_ids.insert(id.to_ascii_lowercase()) {
            continue;
        }
        let Some(mut app) = resolve(&id) else {
            continue;
        };
        if !seen_commands.insert(app.exec.to_ascii_lowercase()) {
            continue;
        }
        app.is_default = default.is_some_and(|value| value.eq_ignore_ascii_case(&id));
        apps.push(app);
    }
    apps
}

fn resolve(classes: &RegKey, id: &str) -> Option<Application> {
    let key = classes.open_subkey(id).ok()?;
    let command = key
        .open_subkey(r"shell\open\command")
        .ok()?
        .get_value::<String, _>("")
        .ok()?;
    if split_command(&command).is_empty() {
        return None;
    }
    let executable = command_executable(&command)?;
    let app_key = classes
        .open_subkey(format!(r"Applications\{executable}"))
        .ok();
    let application_name = key
        .open_subkey("Application")
        .ok()
        .and_then(|application| application.get_value::<String, _>("ApplicationName").ok());
    let registered_app_name = app_key
        .as_ref()
        .and_then(|registered| registered.get_value::<String, _>("FriendlyAppName").ok());
    let registered_name = application_name
        .as_deref()
        .or(registered_app_name.as_deref());
    let direct_name = key.get_value::<String, _>("FriendlyAppName").ok();
    if id.to_ascii_lowercase().starts_with("appx")
        && direct_name
            .as_deref()
            .filter(|name| usable_name(name))
            .is_none()
        && registered_name.filter(|name| usable_name(name)).is_none()
    {
        return None;
    }
    let name = app_name(direct_name.as_deref(), registered_name, &command)?;
    Some(Application {
        id: format!("win:{id}"),
        name,
        exec: command,
        terminal: false,
        is_default: false,
        unavailable: None,
    })
}

fn directory_shell_apps(classes: &RegKey) -> Vec<Application> {
    let Ok(shell) = classes.open_subkey(r"Directory\shell") else {
        return Vec::new();
    };
    shell
        .enum_keys()
        .flatten()
        .filter_map(|verb| {
            let key = shell.open_subkey(&verb).ok()?;
            let command = key
                .open_subkey("command")
                .ok()?
                .get_value::<String, _>("")
                .ok()?;
            let verb_name = key
                .get_value::<String, _>("")
                .ok()
                .filter(|name| usable_name(name))
                .unwrap_or_default();
            if !is_directory_open_handler(&verb_name) {
                return None;
            }
            let name = directory_app_name(&verb_name, &command)?;
            Some(Application {
                id: format!("win:Directory\\shell\\{verb}"),
                name: name.replace('&', ""),
                exec: command,
                terminal: false,
                is_default: false,
                unavailable: None,
            })
        })
        .collect()
}

fn usable_name(name: &str) -> bool {
    !name.trim().is_empty() && !name.starts_with('@')
}

fn is_directory_open_handler(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    normalized.contains("open") || normalized.contains("browse") || normalized.contains("with")
}

fn directory_app_name(verb_name: &str, command: &str) -> Option<String> {
    let clean = verb_name.replace('&', "");
    let label = ["open with ", "browse with "]
        .iter()
        .find_map(|prefix| {
            clean
                .get(..prefix.len())
                .filter(|head| head.eq_ignore_ascii_case(prefix))
                .map(|_| clean[prefix.len()..].trim().to_string())
        })
        .filter(|name| !name.is_empty());
    label.or_else(|| app_name(None, None, command))
}

fn app_name(direct: Option<&str>, registered: Option<&str>, command: &str) -> Option<String> {
    direct
        .filter(|name| usable_name(name))
        .or_else(|| registered.filter(|name| usable_name(name)))
        .map(str::to_string)
        .or_else(|| {
            command_executable(command).and_then(|name| {
                name.rsplit(['\\', '/'])
                    .next()
                    .map(str::to_string)
                    .map(|name| executable_stem(&name).to_string())
                    .filter(|name| !is_generic_command_host(name))
            })
        })
}

fn is_generic_command_host(name: &str) -> bool {
    ["rundll32", "applicationframehost", "openwith"]
        .iter()
        .any(|host| name.eq_ignore_ascii_case(host))
}

fn executable_stem(name: &str) -> &str {
    if name.len() >= 4 && name[name.len() - 4..].eq_ignore_ascii_case(".exe") {
        &name[..name.len() - 4]
    } else {
        name
    }
}

fn command_executable(command: &str) -> Option<String> {
    let executable = split_command(command).into_iter().next()?;
    Some(executable.rsplit(['\\', '/']).next()?.to_string())
}

/// Expands a Windows association's command template into a process command.
pub fn expand_command(template: &str, file: &Path) -> Result<Vec<String>, String> {
    let path = file.to_string_lossy();
    let mut replaced = false;
    let mut args = Vec::new();
    for arg in split_command(template) {
        let mut out = String::new();
        let mut chars = arg.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '%' {
                match chars.peek().copied() {
                    Some('0' | '1' | 'L' | 'l' | 'V' | 'v' | '*') => {
                        chars.next();
                        out.push_str(&path);
                        replaced = true;
                        continue;
                    }
                    Some('%') => {
                        chars.next();
                        out.push('%');
                        continue;
                    }
                    _ => {}
                }
            }
            out.push(c);
        }
        args.push(expand_environment(&out));
    }
    if args.is_empty() {
        return Err("empty Windows application command".into());
    }
    if !replaced {
        args.push(path.into_owned());
    }
    Ok(args)
}

fn expand_environment(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        if let Some(end) = rest[1..].find('%') {
            let end = end + 1;
            let variable = &rest[1..end];
            if let Ok(expanded) = std::env::var(variable) {
                out.push_str(&expanded);
            } else {
                out.push_str(&rest[..=end]);
            }
            rest = &rest[end + 1..];
        } else {
            out.push_str(rest);
            return out;
        }
    }
    out.push_str(rest);
    out
}

fn split_command(command: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if chars.peek() == Some(&'"') => {
                chars.next();
                current.push('"');
                started = true;
            }
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
    if started {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::{Catalog, app_name, assemble, expand_command};
    use crate::apps::Application;
    use std::path::Path;

    #[test]
    fn expands_quoted_windows_paths_and_placeholders() {
        let args = expand_command(
            r#""C:\Program Files\Viewer\viewer.exe" /open "%1""#,
            Path::new(r"C:\My Files\a.txt"),
        )
        .unwrap();
        assert_eq!(
            args,
            [
                r"C:\Program Files\Viewer\viewer.exe",
                "/open",
                r"C:\My Files\a.txt"
            ]
        );
        let args = expand_command(r"viewer.exe /open", Path::new(r"C:\a.txt")).unwrap();
        assert_eq!(args, ["viewer.exe", "/open", r"C:\a.txt"]);
    }

    #[test]
    fn extension_lookup_is_case_insensitive_and_directories_are_separate() {
        let app = Application {
            id: "win:txt".into(),
            name: "Text".into(),
            exec: "text %1".into(),
            terminal: false,
            is_default: true,
            unavailable: None,
        };
        let mut catalog = Catalog::default();
        catalog.by_type.insert(".txt".into(), vec![app.clone()]);
        catalog
            .by_type
            .insert("<directory>".into(), vec![app.clone()]);
        assert_eq!(
            catalog.applications_for("NOTE.TXT", false).as_slice(),
            std::slice::from_ref(&app)
        );
        assert_eq!(catalog.applications_for("folder.txt", true), [app]);
        assert!(catalog.applications_for("note.png", false).is_empty());
    }

    #[test]
    fn default_precedes_candidates_and_duplicate_commands_are_removed() {
        let app = |id: &str| Application {
            id: id.into(),
            name: id.into(),
            exec: if id == "duplicate" || id.eq_ignore_ascii_case("editor") {
                "editor %1"
            } else {
                id
            }
            .into(),
            terminal: false,
            is_default: false,
            unavailable: None,
        };
        let apps = assemble(
            Some("editor"),
            vec![
                "editor".into(),
                "duplicate".into(),
                "other".into(),
                "EDITOR".into(),
            ],
            |id| Some(app(id)),
        );
        assert_eq!(apps.len(), 2);
        assert!(apps[0].is_default);
        assert!(!apps[1].is_default);
    }

    #[test]
    fn app_names_come_from_registration_or_executable_not_progid_description() {
        assert_eq!(
            app_name(
                None,
                Some("Microsoft Edge"),
                r#""C:\Program Files\Edge\msedge.exe" "%1""#
            ),
            Some("Microsoft Edge".into())
        );
        assert_eq!(
            app_name(None, None, r#""C:\Tools\Vivaldi.exe" "%1""#),
            Some("Vivaldi".into())
        );
        assert_eq!(
            app_name(Some("@{random-resource}"), None, "zen.exe %1"),
            Some("zen".into())
        );
    }

    #[test]
    fn directory_shell_entries_keep_open_handlers_and_skip_context_actions() {
        assert!(super::is_directory_open_handler("Open w&ith Positron"));
        assert!(super::is_directory_open_handler("Browse with FastStone"));
        assert!(!super::is_directory_open_handler("AnyCode"));
        assert_eq!(
            super::directory_app_name("Open w&ith &Positron", "Positron.exe %V"),
            Some("Positron".into())
        );
        assert_eq!(
            super::directory_app_name("Browse with FastStone", "FSViewer.exe %V"),
            Some("FastStone".into())
        );
        assert_eq!(
            super::directory_app_name("Open with", "rundll32.exe something"),
            None
        );
        assert_eq!(
            super::app_name(None, None, "ApplicationFrameHost.exe %1"),
            None
        );
        assert_eq!(
            super::app_name(None, None, "FOO.EXE %1"),
            Some("FOO".into())
        );
        assert_eq!(super::app_name(None, None, "RUNDLL32.EXE thing"), None);
    }
}
