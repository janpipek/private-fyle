// `super::*` is safe here: this module does not glob-import gpui, whose
// `test` macro would otherwise shadow `#[test]`.
use std::fs;
use std::path::{Path, PathBuf};

use super::builtin::{GalleryJob, parse_summary};
use super::detect::{detect, remote_url, web_url};
use super::*;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("fyle-tools-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A search path with fake executables of the given names.
#[cfg(unix)]
fn fake_programs(dir: &TempDir, names: &[&str]) -> Programs {
    use std::os::unix::fs::PermissionsExt;
    let bin = dir.0.join("bin");
    fs::create_dir_all(&bin).unwrap();
    for name in names {
        let path = bin.join(name);
        fs::write(&path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    Programs::new(vec![bin])
}

fn ctx(dir: &Path, targets: &[(&str, bool)]) -> ToolContext {
    ToolContext {
        dir: dir.to_path_buf(),
        targets: targets
            .iter()
            .map(|(n, d)| Target::new(dir.join(n), *d))
            .collect(),
        other_dir: Some(PathBuf::from("/other")),
    }
}

fn args(action: &Action) -> Vec<String> {
    match action {
        Action::Run(spec) => spec.args.clone(),
        other => panic!("expected Run, got {other:?}"),
    }
}

// --- detection ---------------------------------------------------------------

#[test]
fn web_urls_from_remotes() {
    let cases = [
        (
            "git@github.com:owner/repo.git",
            Some("https://github.com/owner/repo"),
        ),
        (
            "https://github.com/owner/repo.git",
            Some("https://github.com/owner/repo"),
        ),
        (
            "https://user:token@gitlab.com/g/sub/r",
            Some("https://gitlab.com/g/sub/r"),
        ),
        (
            "ssh://git@host.example:2222/o/r.git",
            Some("https://host.example/o/r"),
        ),
        ("/srv/git/repo.git", None),
        ("C:/repos/x", None),
    ];
    for (remote, expected) in cases {
        assert_eq!(web_url(remote).as_deref(), expected, "{remote}");
    }
}

#[test]
fn origin_wins_over_other_remotes() {
    let config = "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = git@a:u/r\n\
                  [remote \"origin\"]\n\turl = git@b:o/r.git\n";
    assert_eq!(remote_url(config).as_deref(), Some("git@b:o/r.git"));
    assert_eq!(remote_url("[remote \"x\"]\nurl=u").as_deref(), Some("u"));
    assert_eq!(remote_url("[core]\n"), None);
}

#[test]
fn detects_enclosing_projects_and_target_dirs() {
    let tmp = TempDir::new("detect");
    tmp.file(
        ".git/config",
        "[remote \"origin\"]\n\turl = git@github.com:o/r.git\n",
    );
    tmp.file("pyproject.toml", "[project]\nname = \"x\"\n");
    tmp.file("uv.lock", "");
    tmp.file("justfile", "build:\n");
    tmp.file("src/pkg/mod.py", "");
    tmp.file("vendor/lib/Cargo.toml", "");

    let context = ctx(&tmp.0.join("src/pkg"), &[("mod.py", false)]);
    let facts = detect(&context);
    let kinds: Vec<_> = facts.dir.iter().map(Facet::kind).collect();
    assert_eq!(
        kinds,
        [
            FacetKind::GitRepo,
            FacetKind::PythonProject,
            FacetKind::UvProject,
            FacetKind::Justfile
        ]
    );
    assert_eq!(
        facts.dir_facet(FacetKind::GitRepo),
        Some(&Facet::GitRepo {
            root: tmp.0.clone(),
            web_url: Some("https://github.com/o/r".into())
        })
    );
    assert_eq!(facts.targets, [vec![]]);

    // A directory target gets its own facets, without walking up.
    let context = ctx(&tmp.0.join("vendor"), &[("lib", true)]);
    let facts = detect(&context);
    assert_eq!(
        facts.targets,
        [vec![Facet::CargoProject {
            root: tmp.0.join("vendor/lib")
        }]]
    );
}

#[test]
fn uv_detected_from_pyproject_section() {
    let tmp = TempDir::new("uv");
    tmp.file("pyproject.toml", "[tool.uv]\ndev-dependencies = []\n");
    let facts = detect(&ctx(&tmp.0, &[]));
    assert!(facts.dir_facet(FacetKind::UvProject).is_some());
}

// --- programs ------------------------------------------------------------------

#[test]
fn parses_requirements() {
    let req = Requirement::parse("magick|convert (ImageMagick)").unwrap();
    assert_eq!(req.alias, "magick");
    assert_eq!(req.candidates, ["magick", "convert"]);
    assert_eq!(req.hint, "ImageMagick");
    assert_eq!(Requirement::parse("ruff").unwrap().hint, "ruff");
    assert!(Requirement::parse(" (x)").is_err());
}

#[cfg(unix)]
#[test]
fn resolves_alternatives_and_reports_missing() {
    let tmp = TempDir::new("programs");
    let programs = fake_programs(&tmp, &["convert"]);
    let magick = Requirement::parse("magick|convert (ImageMagick)").unwrap();
    let resolved = programs.resolve(&[magick.clone()]).unwrap();
    assert_eq!(resolved["magick"], tmp.0.join("bin/convert"));

    let missing = programs.resolve(&[magick, Requirement::parse("ruff").unwrap()]);
    assert_eq!(missing, Err(vec!["ruff".to_string()]));
}

// --- registry: offers ------------------------------------------------------------

#[cfg(unix)]
#[test]
fn offers_depend_on_targets_and_programs() {
    let tmp = TempDir::new("offers");
    let registry = Registry::with_defaults();
    let programs = fake_programs(&tmp, &["magick", "zip"]);
    let ids = |context: &ToolContext| {
        registry
            .offers(context, &detect(context), &programs)
            .into_iter()
            .map(|o| (o.id, o.missing))
            .collect::<Vec<_>>()
    };

    let one_image = ctx(&tmp.0, &[("a.png", false)]);
    assert_eq!(
        ids(&one_image),
        [
            ("image.resize".into(), vec![]),
            ("image.convert".into(), vec![]),
            ("selection.zip".into(), vec![])
        ]
    );

    let two_images = ctx(&tmp.0, &[("a.png", false), ("b.JPG", false)]);
    let offered: Vec<_> = ids(&two_images).into_iter().map(|(id, _)| id).collect();
    assert_eq!(
        offered,
        [
            "image.gallery",
            "image.resize",
            "image.convert",
            "selection.zip"
        ]
    );

    // Mixed selection: only generic tools.
    let mixed = ctx(&tmp.0, &[("a.png", false), ("x.py", false)]);
    assert_eq!(ids(&mixed), [("selection.zip".into(), vec![])]);

    // Python file with ruff missing: offered but disabled.
    let py = ctx(&tmp.0, &[("x.py", false)]);
    assert_eq!(
        ids(&py)[0],
        ("python.ruff_check".into(), vec!["ruff".to_string()])
    );

    // A directory target: zip (entries = any) but no file tools.
    let dir = ctx(&tmp.0, &[("folder", true)]);
    assert_eq!(ids(&dir), [("selection.zip".into(), vec![])]);
}

#[cfg(unix)]
#[test]
fn directory_scope_tools_follow_the_panel_directory() {
    let tmp = TempDir::new("dirscope");
    tmp.file("pyproject.toml", "");
    tmp.file("uv.lock", "");
    tmp.file("justfile", "");
    let registry = Registry::with_defaults();
    let programs = fake_programs(&tmp, &["uv"]);
    let context = ctx(&tmp.0, &[]); // nothing under the cursor
    let offers = registry.offers(&context, &detect(&context), &programs);
    let summary: Vec<_> = offers
        .iter()
        .map(|o| (o.group.as_str(), o.id.as_str(), o.missing.is_empty()))
        .collect();
    assert_eq!(
        summary,
        [
            ("just", "just.tasks", false),
            ("uv", "uv.sync", true),
            ("uv", "uv.upgrade", true),
            ("uv", "uv.tree", true),
            ("uv", "uv.add", true),
        ]
    );
}

// --- registry: planning ------------------------------------------------------------

#[cfg(unix)]
#[test]
fn plans_one_command_per_target_with_params() {
    let tmp = TempDir::new("plan");
    let registry = Registry::with_defaults();
    let programs = fake_programs(&tmp, &["convert"]);
    let context = ctx(&tmp.0, &[("a.png", false), ("photos/b.jpg", false)]);
    let facts = detect(&context);

    let action = registry
        .plan("image.convert", &context, &facts, &programs, &Values::new())
        .unwrap();
    let Action::Sequence(commands) = action else {
        panic!("expected a sequence");
    };
    let Action::Run(first) = &commands[0] else {
        panic!()
    };
    assert_eq!(first.program, tmp.0.join("bin/convert")); // the alternative
    assert_eq!(
        first.args,
        [
            tmp.0.join("a.png").display().to_string(),
            format!("{}/a.webp", tmp.0.display()) // default parameter
        ]
    );
    assert!(first.refresh);

    let values = Values::from([("format".to_string(), "png".to_string())]);
    let action = registry
        .plan("image.convert", &context, &facts, &programs, &values)
        .unwrap();
    let Action::Sequence(commands) = action else {
        panic!()
    };
    assert_eq!(
        args(&commands[1])[1],
        format!("{}/photos/b.png", tmp.0.display())
    );
}

#[cfg(unix)]
#[test]
fn list_placeholders_expand_to_separate_arguments() {
    let tmp = TempDir::new("zip");
    let registry = Registry::with_defaults();
    let programs = fake_programs(&tmp, &["zip"]);
    let context = ctx(&tmp.0, &[("a b.txt", false), ("dir", true)]);
    let action = registry
        .plan(
            "selection.zip",
            &context,
            &detect(&context),
            &programs,
            &Values::new(),
        )
        .unwrap();
    assert_eq!(
        args(&action),
        ["-r", "/other/archive.zip", "a b.txt", "dir"]
    );
}

#[cfg(unix)]
#[test]
fn planning_refuses_missing_programs_and_wrong_context() {
    let tmp = TempDir::new("refuse");
    let registry = Registry::with_defaults();
    let programs = fake_programs(&tmp, &[]);
    let context = ctx(&tmp.0, &[("x.py", false)]);
    let facts = detect(&context);
    let plan = |id| registry.plan(id, &context, &facts, &programs, &Values::new());
    assert_eq!(plan("python.ruff_check").unwrap_err(), "Requires ruff");
    assert!(plan("image.convert").is_err());
    assert!(plan("nope").is_err());
}

// --- definitions --------------------------------------------------------------------

#[test]
fn user_definitions_validate_and_override() {
    let bad = |run: &str, extra: &str| {
        command::parse(&format!(
            "[[tool]]\nid = \"t\"\ngroup = \"g\"\nlabel = \"l\"\nrun = {run}\n{extra}"
        ))
        .err()
        .unwrap_or_default()
    };
    assert!(bad(r#"["echo", "{nope}"]"#, "").contains("unknown placeholder {nope}"));
    assert!(bad(r#"["echo", "x{paths}"]"#, "").contains("whole argument"));
    assert!(bad(r#"["echo", "{root}"]"#, "").contains("needs `when.facet`"));
    assert!(bad("[]", "").contains("empty `run`"));
    assert!(bad(r#"["echo"]"#, "colour = 1").contains("unknown field"));
    assert!(bad(r#"["echo", "{path"]"#, "").contains("unclosed"));
    assert_eq!(
        bad(r#"["echo", "{path}"]"#, r#"when = { kinds = ["image"] }"#),
        ""
    );

    let mut registry = Registry::with_defaults();
    let before = registry.tools.len();
    registry
        .load_toml(
            "[[tool]]\nid = \"python.ruff_check\"\ngroup = \"Python\"\n\
             label = \"Mine\"\nrun = [\"echo\", \"{paths}\"]\nwhen = { exts = [\"py\"] }",
        )
        .unwrap();
    assert_eq!(registry.tools.len(), before);
    assert_eq!(registry.tool("python.ruff_check").unwrap().label(), "Mine");
}

#[test]
fn defaults_are_valid() {
    let registry = Registry::with_defaults();
    assert!(registry.tool("git.website").is_some());
    assert!(registry.tool("uv.sync").is_some());
}

#[test]
fn conversions_share_one_group() {
    let registry = Registry::with_defaults();
    for id in ["image.convert", "csv.convert"] {
        assert_eq!(registry.tool(id).unwrap().group(), "Convert", "{id}");
    }
}

// --- built-in tools ------------------------------------------------------------------

#[test]
fn repo_website_prefers_target_repository() {
    let context = ctx(Path::new("/w"), &[("lib", true)]);
    let repo = |url: &str| Facet::GitRepo {
        root: PathBuf::from("/x"),
        web_url: Some(url.into()),
    };
    let facts = Facts {
        dir: vec![repo("https://outer")],
        targets: vec![vec![repo("https://inner")]],
    };
    let registry = Registry::with_defaults();
    let action = registry
        .plan(
            "git.website",
            &context,
            &facts,
            &Programs::new(vec![]),
            &Values::new(),
        )
        .unwrap();
    assert!(matches!(action, Action::OpenUrl(url) if url == "https://inner"));
}

#[test]
fn just_summary_and_gallery_html() {
    assert_eq!(
        parse_summary("build test\nlint\n"),
        ["build", "test", "lint"]
    );

    let job = GalleryJob {
        title: "A & B".into(),
        images: vec!["a b.png".into(), "x\"<.jpg".into()],
        output: PathBuf::from("gallery.html"),
    };
    let html = job.html();
    assert!(html.contains("<title>A &amp; B</title>"));
    assert!(html.contains("src=\"a%20b.png\""));
    assert!(html.contains("src=\"x%22%3C.jpg\""));
    assert!(html.contains("<figcaption>x&quot;&lt;.jpg</figcaption>"));
}

// --- execution --------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn runs_commands_and_reports_failures() {
    let spec = |script: &str, output| {
        Action::Run(CommandSpec {
            program: "sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: std::env::temp_dir(),
            output,
            refresh: true,
        })
    };
    let report = exec::run(spec("echo out; echo err >&2", Output::Log)).unwrap();
    assert_eq!(report.log, "out\nerr\n");
    assert!(report.refresh);

    let error = exec::run(spec("echo boom >&2; exit 3", Output::Status)).unwrap_err();
    assert!(error.contains("boom"), "{error}");

    let tmp = TempDir::new("exec");
    let job = GalleryJob {
        title: "t".into(),
        images: vec!["a.png".into()],
        output: tmp.0.join("gallery.html"),
    };
    let report = exec::run(Action::Native(Box::new(job))).unwrap();
    assert!(report.summary.starts_with("Created"));
    assert!(tmp.0.join("gallery.html").is_file());
}

#[test]
fn describes_actions_for_confirmation() {
    let spec = CommandSpec {
        program: PathBuf::from("/usr/bin/magick"),
        args: vec!["a b.png".into(), "out.webp".into(), "it's".into()],
        cwd: PathBuf::from("/"),
        output: Output::Status,
        refresh: false,
    };
    assert_eq!(spec.command_line(), "magick 'a b.png' out.webp 'it'\\''s'");
    let action = Action::Sequence(vec![Action::Run(spec.clone()), Action::Run(spec)]);
    assert_eq!(action.describe().len(), 2);
    assert!(!action.is_harmless());
    assert!(Action::OpenUrl("https://x".into()).is_harmless());
}

// --- Lua plugins -------------------------------------------------------------------

const PLUGIN: &str = r#"
return {
  {
    id = "md.pdf",
    group = "Markdown",
    label = "Convert to PDF…",
    requires = { "pandoc" },
    params = { { key = "engine", label = "Engine", choices = { "typst", "xelatex" } } },
    applies = function(ctx)
      return #ctx.targets == 1 and ctx.targets[1].ext == "md"
    end,
    plan = function(ctx, values)
      local t = ctx.targets[1]
      return { run = { ctx.programs.pandoc, t.path, "--pdf-engine=" .. values.engine,
                       "-o", t.parent .. "/" .. t.stem .. ".pdf" },
               output = "log", refresh = true }
    end,
  },
  {
    id = "where",
    group = "Info",
    label = "Pick a place",
    plan = function(ctx)
      return { pick = { title = "Places", items = {
        { label = "here", action = { open_url = "file://" .. ctx.dir } },
        { label = "other", detail = ctx.other, action = { sequence = {
          { run = { "true" } }, { open_url = "file://" .. ctx.other } } } },
      } } }
    end,
  },
}
"#;

#[cfg(unix)]
#[test]
fn lua_plugins_define_tools() {
    let tmp = TempDir::new("lua");
    let mut registry = Registry::default();
    for tool in lua::load("test.lua", PLUGIN).unwrap() {
        registry.register(Box::new(tool));
    }
    let programs = fake_programs(&tmp, &["pandoc"]);
    let ids = |context: &ToolContext| {
        registry
            .offers(context, &detect(context), &programs)
            .into_iter()
            .map(|o| o.id)
            .collect::<Vec<_>>()
    };
    let md = ctx(&tmp.0, &[("notes.md", false)]);
    assert_eq!(ids(&md), ["md.pdf", "where"]);
    assert_eq!(ids(&ctx(&tmp.0, &[("a.txt", false)])), ["where"]); // applies

    let tool = registry.tool("md.pdf").unwrap();
    assert_eq!(tool.group(), "Markdown");
    assert_eq!(tool.params(&md)[0].default_value(), "typst");

    let facts = detect(&md);
    let Action::Run(spec) = registry
        .plan("md.pdf", &md, &facts, &programs, &Values::new())
        .unwrap()
    else {
        panic!("expected Run");
    };
    assert_eq!(spec.program, tmp.0.join("bin/pandoc"));
    let parent = tmp.0.display();
    assert_eq!(
        spec.args,
        [
            format!("{parent}/notes.md"),
            "--pdf-engine=typst".into(),
            "-o".into(),
            format!("{parent}/notes.pdf"),
        ]
    );
    assert_eq!(
        (spec.output, spec.refresh, spec.cwd.clone()),
        (Output::Log, true, tmp.0.clone())
    );

    let action = registry
        .plan("where", &md, &facts, &programs, &Values::new())
        .unwrap();
    assert_eq!(action.describe(), ["Places: 2 choices"]);
    let Action::Pick { items, .. } = action else {
        panic!()
    };
    assert_eq!(items[1].detail.as_deref(), Some("/other"));
    assert_eq!(items[1].action.describe(), ["true", "Open file:///other"]);
}

#[test]
fn lua_plugin_errors_are_reported() {
    let err = |source: &str| match lua::load("bad.lua", source) {
        Ok(_) => panic!("loaded {source:?}"),
        Err(e) => e,
    };
    assert!(err("return {").starts_with("bad.lua:"));
    assert!(
        err("return { { group = 'g', label = 'l', plan = function() end } }")
            .contains("missing `id`")
    );
    assert!(err("return { { id = 'x', group = 'g', label = 'l' } }").contains("`plan`"));

    // Runtime errors surface when planning, with the tool id.
    let tools = lua::load(
        "t.lua",
        "return { { id = 'x', group = 'g', label = 'l', plan = function() error('nope') end },
                  { id = 'y', group = 'g', label = 'l', plan = function() return {} end } }",
    )
    .unwrap();
    let mut registry = Registry::default();
    for tool in tools {
        registry.register(Box::new(tool));
    }
    let context = ctx(Path::new("/tmp"), &[]);
    let plan = |id| {
        registry
            .plan(
                id,
                &context,
                &Facts::default(),
                &Programs::new(vec![]),
                &Values::new(),
            )
            .unwrap_err()
    };
    assert!(plan("x").contains("nope"), "{}", plan("x"));
    assert!(plan("y").contains("needs one of run"), "{}", plan("y"));
}

#[test]
fn lua_plugins_load_from_a_directory() {
    let tmp = TempDir::new("luadir");
    tmp.file(
        "b.lua",
        "return { { id = 'b', group = 'g', label = 'B', plan = function() end } }",
    );
    tmp.file(
        "a.lua",
        "return { { id = 'a', group = 'g', label = 'A', plan = function() end } }",
    );
    tmp.file("broken.lua", "this is not lua");
    tmp.file("notes.txt", "ignored");
    let (tools, errors) = lua::load_dir(&tmp.0);
    let ids: Vec<_> = tools.iter().map(|t| t.id().to_string()).collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("broken.lua:"));
    assert!(lua::load_dir(&tmp.0.join("missing")).0.is_empty());
}

#[test]
fn offers_are_grouped_in_registry_order_with_missing_details_intact() {
    let offer = |id: &str, group: &str, missing: &[&str]| Offer {
        id: id.into(),
        group: group.into(),
        label: id.to_uppercase(),
        missing: missing.iter().map(|item| item.to_string()).collect(),
    };
    let groups = group_offers(vec![
        offer("a", "Project", &[]),
        offer("b", "Project", &["just"]),
        offer("c", "Convert", &["ImageMagick"]),
    ]);

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].label, "Project");
    assert_eq!(
        groups[0]
            .offers
            .iter()
            .map(|o| o.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(groups[0].offers[1].missing, ["just"]);
    assert_eq!(groups[1].label, "Convert");
}
