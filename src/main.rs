// Release builds on Windows should not open a console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod columns;
mod commander;
mod editor;
mod editor_view;
mod fs_ops;
mod git_status;
mod icons;
mod jobs;
mod menu_bar;
mod menus;
mod model;
mod openers;
mod palette;
mod palette_view;
mod panel;
mod places;
mod preview;
mod preview_view;
mod rename;
mod search;
mod settings;
mod terminal;
mod theme_picker;
mod themes;
mod tools;
mod tools_view;
mod xattr;

use std::path::PathBuf;

use gpui_kit::*;

use commander::Commander;
use settings::Settings;
use themes::ThemeChoice;

fn main() {
    let (settings, settings_error) = Settings::load();

    // Command-line directories override the configured ones.
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let left = args
        .next()
        .or_else(|| settings.panels.left.clone())
        .unwrap_or_else(|| cwd.clone());
    let right = args
        .next()
        .or_else(|| settings.panels.right.clone())
        .or_else(std::env::home_dir)
        .unwrap_or(cwd);

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            commander::bind_keys(cx);
            editor_view::bind_keys(cx);
            panel::bind_keys(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let size = size(px(settings.window.width), px(settings.window.height));
            let bounds = Bounds::centered(None, size, cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Fyle".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let mut errors: Vec<String> = settings_error.into_iter().collect();
            errors.extend(commander::bind_user_keys(&settings.keymap, cx));
            errors.extend(themes::register_all(cx));
            gpui_kit::open_window(options, cx, |window, cx| {
                let choice = ThemeChoice::parse(&settings.theme);
                if let Err(e) = themes::apply(&choice, window, cx) {
                    errors.push(e);
                }
                let error = (!errors.is_empty()).then(|| errors.join("; "));
                let commander =
                    cx.new(|cx| Commander::new([left, right], &settings, error, window, cx));
                commander.update(cx, |commander, cx| commander.init(window, cx));
                commander
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
