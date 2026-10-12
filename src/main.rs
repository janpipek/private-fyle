// Release builds on Windows should not open a console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

// The Linux association modules stay compiled on Windows for their model tests.
#[cfg_attr(windows, allow(dead_code))]
mod apps;
mod archive;
mod bookmarks;
mod breadcrumbs;
mod columns;
mod commander;
mod conflict_view;
mod context_menu;
mod copy_path;
#[cfg_attr(windows, allow(dead_code))]
mod desktop;
mod dirsize;
mod editor;
mod editor_view;
mod entry;
mod formats;
mod fs_ops;
mod git_status;
mod glyphs;
mod grid;
mod icon_view;
mod icons;
#[cfg_attr(windows, allow(dead_code))]
mod ini;
mod jobs;
mod keymap;
mod link_badge;
mod links;
mod listing;
mod location;
mod location_codec;
mod location_view;
mod menu_bar;
mod menus;
#[cfg_attr(windows, allow(dead_code))]
mod mime;
#[cfg_attr(windows, allow(dead_code))]
mod mimeapps;
mod model;
mod navigation;
mod notebook_view;
mod openers;
mod palette;
mod palette_view;
mod panel;
mod panels;
mod places;
mod preview;
mod preview_view;
mod raw_preview;
mod remote;
mod remote_labels;
mod rename;
mod search;
mod settings;
mod terminal;
mod theme_picker;
mod themes;
mod thumb_queue;
mod thumbnails;
mod tools;
mod tools_view;
mod transfer;
#[cfg(windows)]
mod windows_apps;
pub mod workspace;
mod xattr;
#[cfg_attr(windows, allow(dead_code))]
mod xdg;
mod zoom;

use std::path::PathBuf;

use gpui_kit::*;

use commander::Commander;
use themes::ThemeChoice;

fn main() {
    let (settings, settings_error) = settings::persistence::load();

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
        .unwrap_or_else(|| cwd.clone());
    // Breadcrumbs, history and Go to need absolute paths ("fyle src" → /…/src).
    // `absolute` preserves the user's symlink and `..` spelling; canonicalize
    // here would change the location identity before the panel lists it.
    let [left, right] = [left, right].map(|path| {
        if location::is_remote(&path) {
            return path;
        }
        std::path::absolute(&path).unwrap_or_else(|_| {
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
        })
    });

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            commander::bind_keys(cx);
            editor_view::bind_keys(cx);
            panel::bind_keys(cx);
            icon_view::bind_keys(cx);
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
            if let Err(keymap_errors) = commander::validate_user_keys(&settings.keymap, cx) {
                errors.extend(keymap_errors);
            }
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
                errors.extend(commander::bind_user_keys(&settings.keymap, cx));
                commander
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
