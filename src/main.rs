// Release builds on Windows should not open a console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod commander;
mod fs_ops;
mod model;
mod panel;
mod preview;
mod preview_view;
mod settings;

use std::path::PathBuf;

use gpui_kit::*;

use commander::Commander;
use gpui_kit::component::{Theme, ThemeMode};
use settings::{Settings, ThemeSetting};

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
            gpui_kit::open_window(options, cx, |window, cx| {
                match settings.theme {
                    ThemeSetting::System => Theme::sync_system_appearance(Some(window), cx),
                    ThemeSetting::Light => Theme::change(ThemeMode::Light, Some(window), cx),
                    ThemeSetting::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
                }
                let commander = cx
                    .new(|cx| Commander::new([left, right], &settings, settings_error, window, cx));
                commander.update(cx, |commander, cx| commander.init(window, cx));
                commander
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
