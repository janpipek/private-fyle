//! Applications that can open a file or directory: desktop associations
//! (Linux: shared-mime-info, desktop entries, `mimeapps.list`) merged with
//! fyle's configured openers; UI-independent. Linux and Windows associations
//! are discovered behind [`Catalog`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::desktop::{self, DesktopEntry};
use crate::ini::Ini;
use crate::mime::MimeDb;
use crate::mimeapps::MimeApps;
use crate::openers::Opener;
use crate::xdg::Env;

/// An application registered for a file type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Application {
    /// Desktop file ID.
    pub id: String,
    pub name: String,
    /// The desktop entry's `Exec` line (with field codes).
    pub exec: String,
    /// Runs in a terminal window.
    pub terminal: bool,
    /// The system's default application for the file's type.
    pub is_default: bool,
    /// Why it cannot run (its program is missing), if so.
    pub unavailable: Option<String>,
}

impl Application {
    /// Program (by file name) and fixed arguments, without file codes: what
    /// makes two entries "the same command".
    fn command_key(&self) -> Option<(String, Vec<String>)> {
        #[cfg(windows)]
        if self.id.starts_with("win:") {
            let placeholder = Path::new(r"C:\fyle-open-with-placeholder");
            let tokens = crate::windows_apps::expand_command(&self.exec, placeholder).ok()?;
            let mut tokens = tokens.into_iter();
            let program = base_name(&tokens.next()?).to_ascii_lowercase();
            return Some((
                program,
                tokens
                    .filter(|arg| arg != &placeholder.to_string_lossy())
                    .collect(),
            ));
        }
        let mut tokens = desktop::split_exec(&self.exec).ok()?.into_iter();
        let program = base_name(&tokens.next()?).to_string();
        Some((program, tokens.filter(|t| !t.contains('%')).collect()))
    }

    /// The command line opening `file`.
    pub fn command_line(&self, file: &Path) -> Result<Vec<String>, String> {
        #[cfg(windows)]
        if self.id.starts_with("win:") {
            return crate::windows_apps::expand_command(&self.exec, file)
                .map_err(|e| format!("{}: {e}", self.name));
        }
        desktop::expand_exec(&self.exec, &[file]).map_err(|e| format!("{}: {e}", self.name))
    }
}

/// Everything discovered on disk: MIME database, desktop entries and
/// associations. Loaded once in the background and reused for a while.
#[derive(Debug, Default)]
pub struct Catalog {
    mime: MimeDb,
    entries: HashMap<String, DesktopEntry>,
    /// MIME type to desktop file IDs declaring it in `MimeType=`.
    declared: HashMap<String, Vec<String>>,
    mimeapps: MimeApps,
    #[cfg(windows)]
    windows: Option<crate::windows_apps::Catalog>,
}

/// Directories scanned below `applications/`, as a guard against loops.
const MAX_DEPTH: usize = 6;

impl Catalog {
    /// Loads associations for the current platform off the UI thread.
    pub fn load_platform() -> Self {
        #[cfg(windows)]
        {
            Self {
                windows: Some(crate::windows_apps::Catalog::load()),
                ..Self::default()
            }
        }
        #[cfg(not(windows))]
        Self::load(&Env::from_env())
    }

    /// Reads the databases in `env`; slow, so call it off the UI thread.
    pub fn load(env: &Env) -> Self {
        let data_dirs = env.all_data_dirs();
        let mut catalog = Self {
            mime: MimeDb::load(&data_dirs),
            mimeapps: MimeApps::load(&env.mimeapps_paths()),
            ..Self::default()
        };
        // Earlier directories win for an ID; a Hidden entry deletes it.
        let mut seen: HashSet<String> = HashSet::new();
        for dir in &data_dirs {
            let root = dir.join("applications");
            let mut files = Vec::new();
            collect_desktop_files(&root, 0, &mut files);
            files.sort();
            for file in files {
                let Some(id) = desktop::desktop_id(&root, &file) else {
                    continue;
                };
                if !seen.insert(id.clone()) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&file) else {
                    continue;
                };
                if desktop_visible(&text, &env.desktop_names)
                    && let Some(entry) = desktop::parse(&id, &file, &text, &env.locales)
                    && !entry.hidden
                {
                    catalog.insert(entry);
                }
            }
        }
        catalog
    }

    /// Program names used by loaded desktop entries' `Exec` and `TryExec` fields.
    pub fn program_names(&self) -> HashSet<String> {
        #[cfg(windows)]
        if self.windows.is_some() {
            return HashSet::new();
        }
        self.entries
            .values()
            .flat_map(|entry| {
                desktop::exec_program(&entry.exec)
                    .into_iter()
                    .chain(entry.try_exec.as_deref().and_then(desktop::exec_program))
            })
            .collect()
    }

    fn insert(&mut self, entry: DesktopEntry) {
        for mime in &entry.mime_types {
            let mime = self.mime.canonical(mime).to_string();
            self.declared
                .entry(mime)
                .or_default()
                .push(entry.id.clone());
        }
        self.entries.insert(entry.id.clone(), entry);
    }

    /// The applications for a file name, default first, then the explicitly
    /// associated ones, then those declaring the type, each group sorted
    /// as found (associations) or by name. A more specific type's
    /// applications come before those of its parents (`text/x-rust`, then
    /// `text/plain`). `exists` tells whether a program can be found.
    pub fn applications_for(
        &self,
        file_name: &str,
        is_dir: bool,
        exists: &dyn Fn(&str) -> bool,
    ) -> Vec<Application> {
        #[cfg(windows)]
        if let Some(windows) = &self.windows {
            return windows.applications_for(file_name, is_dir);
        }
        let types = self
            .mime
            .with_ancestors(&self.mime.detect(file_name, is_dir));
        let explicit_default = types
            .iter()
            .flat_map(|t| self.mimeapps.defaults(t))
            .find(|id| self.entries.contains_key(id));

        let mut order: Vec<String> = explicit_default.iter().cloned().collect();
        let mut explicit: HashSet<String> = order.iter().cloned().collect();
        for mime in &types {
            let removed = self.mimeapps.removed(mime);
            let added = self.mimeapps.added(mime);
            explicit.extend(added.iter().cloned());
            let mut declared: Vec<&String> = self
                .declared
                .get(mime)
                .into_iter()
                .flatten()
                .filter(|id| !removed.contains(*id) && !added.contains(*id))
                .collect();
            declared.sort_by_key(|id| self.entries.get(*id).map(|e| e.name.to_lowercase()));
            order.extend(added);
            order.extend(declared.into_iter().cloned());
        }

        let mut seen = HashSet::new();
        let mut apps: Vec<Application> = order
            .into_iter()
            .filter(|id| seen.insert(id.clone()))
            .filter_map(|id| self.entries.get(&id))
            .filter(|e| explicit.contains(&e.id) || !e.no_display)
            .map(|e| application(e, exists))
            .collect();
        // Without an explicit default the first association stands in, as
        // the spec suggests.
        let default_ix = explicit_default
            .and_then(|id| apps.iter().position(|a| a.id == id))
            .or_else(|| (!apps.is_empty()).then_some(0));
        if let Some(app) = default_ix.and_then(|ix| apps.get_mut(ix)) {
            app.is_default = true;
        }
        apps
    }
}

/// Whether a desktop entry applies to the current desktop environment.
/// Desktop names are case-sensitive and a colon-separated environment can
/// match any listed name.
fn desktop_visible(text: &str, desktops: &[String]) -> bool {
    let ini = Ini::parse(text);
    let values = |key| {
        ini.get("Desktop Entry", key)
            .unwrap_or_default()
            .split(';')
            .filter(|name| !name.is_empty())
            .map(String::from)
            .collect::<Vec<_>>()
    };
    let only = values("OnlyShowIn");
    let not = values("NotShowIn");
    for desktop in desktops {
        if only.contains(desktop) {
            return true;
        }
        if not.contains(desktop) {
            return false;
        }
    }
    !ini.section("Desktop Entry")
        .iter()
        .any(|(key, _)| key == "OnlyShowIn")
}

fn application(entry: &DesktopEntry, exists: &dyn Fn(&str) -> bool) -> Application {
    let program = desktop::exec_program(&entry.exec);
    let try_exec = entry.try_exec.as_deref().and_then(desktop::exec_program);
    let missing = match (&try_exec, &program) {
        (Some(try_exec), _) if !exists(try_exec) => Some(try_exec.clone()),
        (_, Some(program)) if !exists(program) => Some(program.clone()),
        (_, None) => Some("a valid Exec".to_string()),
        _ => None,
    };
    Application {
        id: entry.id.clone(),
        name: entry.name.clone(),
        exec: entry.exec.clone(),
        terminal: entry.terminal,
        is_default: false,
        unavailable: missing.map(|m| format!("needs {m}")),
    }
}

fn collect_desktop_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        // `metadata` follows symlinks, which distro packages use a lot.
        match std::fs::metadata(&path) {
            Ok(m) if m.is_dir() => collect_desktop_files(&path, depth + 1, out),
            Ok(m) if m.is_file() && path.extension().is_some_and(|e| e == "desktop") => {
                out.push(path)
            }
            _ => {}
        }
    }
}

/// One line of the Open With list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// A configured `[[opener]]`; marked when it runs the system default.
    Opener {
        opener: Opener,
        is_default: bool,
    },
    App(Application),
    /// Hand the file to `xdg-open` and the like, when no default is known.
    SystemDefault,
    BuiltInEditor,
}

/// The kind of group a [`Choice`] is listed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Configured,
    System,
    Other,
}

impl Group {
    /// The heading shown above the group.
    pub fn title(self) -> &'static str {
        match self {
            Self::Configured => "Configured openers",
            Self::System => "System applications",
            Self::Other => "Other",
        }
    }
}

impl Choice {
    /// The list text, with "(default)" on the system default.
    pub fn label(&self) -> String {
        let marked = |name: &str, is_default: bool| {
            if is_default {
                format!("{name} (default)")
            } else {
                name.to_string()
            }
        };
        match self {
            Self::Opener { opener, is_default } => marked(&opener.label, *is_default),
            Self::App(app) => marked(&app.name, app.is_default),
            Self::SystemDefault => "System default".into(),
            Self::BuiltInEditor => "Built-in editor (F4)".into(),
        }
    }

    pub fn group(&self) -> Group {
        match self {
            Self::Opener { .. } => Group::Configured,
            Self::App(_) | Self::SystemDefault => Group::System,
            Self::BuiltInEditor => Group::Other,
        }
    }

    /// Why this cannot be chosen.
    pub fn unavailable(&self) -> Option<&str> {
        match self {
            Self::App(app) => app.unavailable.as_deref(),
            _ => None,
        }
    }
}

fn base_name(program: &str) -> &str {
    Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program)
}

/// Orders the choices: matching configured openers, then the system's
/// default and other applications, then the built-in editor when it can open
/// the file. A system application running the same command as a configured
/// opener is dropped, and the opener takes its "(default)" mark.
/// `SystemDefault` is added only when no default application is known.
pub fn merge(openers: Vec<Opener>, apps: Vec<Application>, include_editor: bool) -> Vec<Choice> {
    let mut apps = apps;
    let mut choices = Vec::new();
    let mut default_known = false;
    for opener in openers {
        let key = opener.command.split_first().map(|(program, args)| {
            let args: Vec<String> = args
                .iter()
                .filter(|a| !a.contains("{path}"))
                .cloned()
                .collect();
            let program = base_name(program).to_string();
            #[cfg(windows)]
            let program = program.to_ascii_lowercase();
            (program, args)
        });
        let mut is_default = false;
        if let Some(key) = key {
            apps.retain(|app| {
                let same = app.command_key().as_ref() == Some(&key);
                is_default |= same && app.is_default;
                !same
            });
        }
        default_known |= is_default;
        choices.push(Choice::Opener { opener, is_default });
    }
    default_known |= apps.iter().any(|a| a.is_default);
    choices.extend(apps.into_iter().map(Choice::App));
    if !default_known {
        choices.push(Choice::SystemDefault);
    }
    if include_editor {
        choices.push(Choice::BuiltInEditor);
    }
    choices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, text: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn desktop(name: &str, exec: &str, extra: &str) -> String {
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n{extra}")
    }

    /// A fake system with one user and one system directory.
    struct Fixture {
        _tmp: tempfile::TempDir,
        env: Env,
    }

    fn fixture(desktops: &[&str], setup: impl FnOnce(&Path, &Path, &Path)) -> Fixture {
        let tmp = tempfile::TempDir::new().unwrap();
        let (config, home, sys) = (
            tmp.path().join("config"),
            tmp.path().join("data"),
            tmp.path().join("usr/share"),
        );
        write(
            &sys,
            "mime/globs2",
            "50:text/x-rust:*.rs\n50:text/plain:*.txt\n50:image/png:*.png\n",
        );
        write(&sys, "mime/subclasses", "text/x-rust text/plain\n");
        setup(&config, &home, &sys);
        let env = Env {
            config_home: Some(config),
            config_dirs: Vec::new(),
            data_home: Some(home),
            data_dirs: vec![sys],
            desktops: desktops.iter().map(|d| d.to_lowercase()).collect(),
            desktop_names: desktops.iter().map(|d| d.to_string()).collect(),
            locales: Vec::new(),
        };
        Fixture { _tmp: tmp, env }
    }

    fn names(apps: &[Application]) -> Vec<String> {
        apps.iter()
            .map(|a| {
                let mark = if a.is_default { "*" } else { "" };
                format!("{}{mark}", a.id)
            })
            .collect()
    }

    fn all_exist(_: &str) -> bool {
        true
    }

    #[test]
    fn default_added_declared_order_with_parent_types() {
        let fx = fixture(&[], |config, _, sys| {
            write(
                sys,
                "applications/rustide.desktop",
                &desktop("RustIDE", "rustide %f", "MimeType=text/x-rust;"),
            );
            write(
                sys,
                "applications/zed.desktop",
                &desktop("Zed", "zed %f", "MimeType=text/x-rust;text/plain;"),
            );
            write(
                sys,
                "applications/gedit.desktop",
                &desktop("Gedit", "gedit %f", "MimeType=text/plain;"),
            );
            write(
                sys,
                "applications/atom.desktop",
                &desktop("Atom", "atom %f", "MimeType=text/plain;"),
            );
            write(
                sys,
                "applications/viewer.desktop",
                &desktop("Viewer", "v %f", "MimeType=image/png;"),
            );
            write(
                config,
                "mimeapps.list",
                "[Default Applications]\ntext/x-rust=zed.desktop\n\
                [Added Associations]\ntext/plain=gedit.desktop;\n",
            );
        });
        let catalog = Catalog::load(&fx.env);
        let apps = catalog.applications_for("main.rs", false, &all_exist);
        // default; rust's own (RustIDE); then text/plain: added gedit, rest by name (Atom).
        assert_eq!(
            names(&apps),
            [
                "zed.desktop*",
                "rustide.desktop",
                "gedit.desktop",
                "atom.desktop"
            ]
        );
        let png = catalog.applications_for("a.PNG", false, &all_exist);
        assert_eq!(names(&png), ["viewer.desktop*"]);
        assert!(
            catalog
                .applications_for("a.zzz", false, &all_exist)
                .is_empty()
        );
    }

    #[test]
    fn program_names_include_exec_and_try_exec() {
        let fx = fixture(&[], |_, _, sys| {
            write(
                sys,
                "applications/editor.desktop",
                &desktop(
                    "Editor",
                    "editor-bin %f",
                    "TryExec=editor-check\nMimeType=text/plain;",
                ),
            );
        });
        let names = Catalog::load(&fx.env).program_names();
        assert!(names.contains("editor-bin"));
        assert!(names.contains("editor-check"));
    }

    #[test]
    fn directories_use_directory_associations_even_with_file_like_names() {
        let fx = fixture(&[], |config, _, sys| {
            write(
                sys,
                "applications/files.desktop",
                &desktop("Files", "files %f", "MimeType=inode/directory;"),
            );
            write(
                sys,
                "applications/editor.desktop",
                &desktop("Editor", "editor %f", "MimeType=text/plain;"),
            );
            write(
                config,
                "mimeapps.list",
                "[Default Applications]\ninode/directory=files.desktop\n",
            );
        });
        let catalog = Catalog::load(&fx.env);
        assert_eq!(
            names(&catalog.applications_for("notes.txt", true, &all_exist)),
            ["files.desktop*"]
        );
        assert_eq!(
            names(&catalog.applications_for("notes.txt", false, &all_exist)),
            ["editor.desktop*"]
        );
    }

    #[test]
    fn removed_hidden_and_nodisplay() {
        let fx = fixture(&[], |config, home, sys| {
            for (id, extra) in [
                ("a", ""),
                ("b", ""),
                ("c", "NoDisplay=true\n"),
                ("d", ""),
                ("e", "NoDisplay=true\n"),
            ] {
                write(
                    sys,
                    &format!("applications/{id}.desktop"),
                    &desktop(
                        id,
                        &format!("{id} %f"),
                        &format!("MimeType=text/plain;\n{extra}"),
                    ),
                );
            }
            // The user's directory hides d and overrides b's name.
            write(
                home,
                "applications/d.desktop",
                "[Desktop Entry]\nType=Application\nName=d\nExec=d\nHidden=true\n",
            );
            write(
                config,
                "mimeapps.list",
                "[Removed Associations]\ntext/plain=b.desktop;\n\
                [Default Applications]\ntext/plain=c.desktop\n[Added Associations]\ntext/plain=e.desktop\n",
            );
        });
        let apps = Catalog::load(&fx.env).applications_for("n.txt", false, &all_exist);
        // c is NoDisplay but default; e is NoDisplay but explicitly added; b removed; d hidden.
        assert_eq!(names(&apps), ["c.desktop*", "e.desktop", "a.desktop"]);
    }

    #[test]
    fn desktop_specific_list_wins_and_subdirectory_ids() {
        let fx = fixture(&["kde", "plasma"], |config, _, sys| {
            write(
                sys,
                "applications/kde4/kate.desktop",
                &desktop("Kate", "kate %f", "MimeType=text/plain;"),
            );
            write(
                sys,
                "applications/gedit.desktop",
                &desktop("Gedit", "gedit %f", "MimeType=text/plain;"),
            );
            write(
                config,
                "mimeapps.list",
                "[Default Applications]\ntext/plain=gedit.desktop\n",
            );
            write(
                config,
                "kde-mimeapps.list",
                "[Default Applications]\ntext/plain=missing.desktop;kde4-kate.desktop\n",
            );
        });
        let apps = Catalog::load(&fx.env).applications_for("n.txt", false, &all_exist);
        assert_eq!(names(&apps), ["kde4-kate.desktop*", "gedit.desktop"]);
    }

    #[test]
    fn only_show_in_and_not_show_in_filter_by_current_desktop() {
        let fx = fixture(&["KDE", "Plasma"], |_, _, sys| {
            for (id, visibility) in [
                ("only-kde", "OnlyShowIn=KDE;"),
                ("only-gnome", "OnlyShowIn=GNOME;"),
                ("not-kde", "NotShowIn=KDE;"),
                ("not-gnome", "NotShowIn=GNOME;"),
                ("case-sensitive", "OnlyShowIn=kde;"),
                ("unrestricted", ""),
            ] {
                write(
                    sys,
                    &format!("applications/{id}.desktop"),
                    &desktop(
                        id,
                        &format!("{id} %f"),
                        &format!("MimeType=text/plain;\n{visibility}\n"),
                    ),
                );
            }
        });
        let apps = Catalog::load(&fx.env).applications_for("n.txt", false, &all_exist);
        assert_eq!(
            names(&apps),
            [
                "not-gnome.desktop*",
                "only-kde.desktop",
                "unrestricted.desktop"
            ]
        );
    }

    #[test]
    fn only_show_in_does_not_match_when_desktop_is_unknown_or_empty() {
        for desktops in [&[][..], &["GNOME"][..]] {
            let fx = fixture(desktops, |_, _, sys| {
                write(
                    sys,
                    "applications/only.desktop",
                    &desktop("Only", "only %f", "MimeType=text/plain;\nOnlyShowIn=KDE;\n"),
                );
            });
            assert!(
                Catalog::load(&fx.env)
                    .applications_for("n.txt", false, &all_exist)
                    .is_empty()
            );
        }
    }

    #[test]
    fn conflicting_visibility_matches_use_desktop_order() {
        let visibility = "OnlyShowIn=KDE;\nNotShowIn=GNOME;";
        assert!(desktop_visible(
            &desktop("App", "app", visibility),
            &["KDE".into(), "GNOME".into()]
        ));
        assert!(!desktop_visible(
            &desktop("App", "app", visibility),
            &["GNOME".into(), "KDE".into()]
        ));
    }

    #[test]
    fn first_association_stands_in_for_a_missing_default() {
        let fx = fixture(&[], |_, _, sys| {
            write(
                sys,
                "applications/b.desktop",
                &desktop("Beta", "b %f", "MimeType=text/plain;"),
            );
            write(
                sys,
                "applications/a.desktop",
                &desktop("Alpha", "a %f", "MimeType=text/plain;"),
            );
        });
        let apps = Catalog::load(&fx.env).applications_for("n.txt", false, &all_exist);
        assert_eq!(names(&apps), ["a.desktop*", "b.desktop"]);
    }

    #[test]
    fn missing_programs_are_marked_unavailable() {
        let fx = fixture(&[], |_, _, sys| {
            write(
                sys,
                "applications/a.desktop",
                &desktop("A", "/opt/a/bin/a %f", "MimeType=text/plain;"),
            );
            write(
                sys,
                "applications/b.desktop",
                &desktop("B", "b %f", "MimeType=text/plain;\nTryExec=btool\n"),
            );
            write(
                sys,
                "applications/c.desktop",
                &desktop("C", "c %f", "MimeType=text/plain;"),
            );
        });
        let exists = |p: &str| p == "c";
        let apps = Catalog::load(&fx.env).applications_for("n.txt", false, &exists);
        let note = |ix: usize| apps[ix].unavailable.clone();
        assert_eq!(note(0).as_deref(), Some("needs /opt/a/bin/a"));
        assert_eq!(note(1).as_deref(), Some("needs btool"));
        assert_eq!(note(2), None);
        assert_eq!(
            apps[2].command_line(Path::new("/x y")).unwrap(),
            ["c", "/x y"]
        );
    }

    fn opener(label: &str, program: &str) -> Opener {
        Opener {
            label: label.into(),
            command: vec![program.into()],
            exts: Vec::new(),
            kinds: Vec::new(),
            names: Vec::new(),
        }
    }

    fn app(id: &str, exec: &str, is_default: bool) -> Application {
        Application {
            id: id.into(),
            name: id.into(),
            exec: exec.into(),
            terminal: false,
            is_default,
            unavailable: None,
        }
    }

    fn labels(choices: &[Choice]) -> Vec<String> {
        choices.iter().map(Choice::label).collect()
    }

    #[test]
    fn merge_orders_and_dedupes_by_command() {
        let choices = merge(
            vec![
                opener("Mine", "/usr/bin/gedit"),
                opener("Other", "zathura"),
                opener("Code", "code"),
            ],
            vec![
                app("kate", "kate %f", false),
                app("gedit", "gedit %U", true),
                app("vim", "vim %f", false),
                app("code", "code --new-window %F", false),
            ],
            true,
        );
        assert_eq!(
            labels(&choices),
            [
                "Mine (default)",
                "Other",
                "Code",
                "kate",
                "vim",
                "code",
                "Built-in editor (F4)"
            ]
        );
        assert_eq!(choices[0].group(), Group::Configured);
        assert_eq!(choices[3].group(), Group::System);
        assert_eq!(choices[6].group(), Group::Other);
    }

    #[test]
    fn merge_marks_the_app_default_and_offers_system_default_only_without_one() {
        let with = merge(vec![], vec![app("kate", "kate %f", true)], true);
        assert_eq!(labels(&with), ["kate (default)", "Built-in editor (F4)"]);
        let without = merge(
            vec![opener("Mine", "x")],
            vec![app("kate", "kate %f", false)],
            true,
        );
        assert_eq!(
            labels(&without),
            ["Mine", "kate", "System default", "Built-in editor (F4)"]
        );
    }

    #[test]
    fn directory_choices_have_no_editor() {
        let choices = merge(vec![], vec![app("Files", "files %f", true)], false);
        assert_eq!(labels(&choices), ["Files (default)"]);
    }

    #[test]
    fn binary_file_choices_have_no_editor() {
        let choices = merge(vec![], vec![app("Viewer", "viewer %f", true)], false);
        assert_eq!(labels(&choices), ["Viewer (default)"]);
    }
}
