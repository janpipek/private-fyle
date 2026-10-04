mod commander;
mod fs_ops;

use std::path::PathBuf;

use gpui_kit::*;

use commander::Commander;

fn main() {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let left = args.next().unwrap_or_else(|| cwd.clone());
    let right = args
        .next()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or(cwd);

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            commander::bind_keys(cx);

            let bounds = Bounds::centered(None, size(px(1100.), px(700.)), cx);
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
