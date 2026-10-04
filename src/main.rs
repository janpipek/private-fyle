// Release builds on Windows should not open a console window.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod commander;
mod fs_ops;
mod model;
mod panel;

use std::path::PathBuf;

use gpui_kit::*;

use commander::Commander;

fn main() {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let left = args.next().unwrap_or_else(|| cwd.clone());
    let right = args.next().or_else(std::env::home_dir).unwrap_or(cwd);

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

            let bounds = Bounds::centered(None, size(px(1200.), px(760.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Fyle".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Commander::new(left, right, window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
