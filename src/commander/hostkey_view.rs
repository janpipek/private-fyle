//! Explicit SSH trust confirmation shared by navigation and connection tests.

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

/// Prompts for an unknown key, saves it off the UI thread, then reports the result.
pub fn confirm(
    key: crate::remote::hostkey::UnknownKey,
    window: &mut Window,
    cx: &mut App,
    after: impl Fn(std::io::Result<()>, &mut Window, &mut App) + 'static,
) {
    let after = std::rc::Rc::new(after);
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let key = key.clone();
        let after = after.clone();
        dialog
            .title("Unknown SSH host key")
            .description(key.description())
            .ok_text("Accept and connect")
            .show_cancel(true)
            .on_ok(move |_, window, cx| {
                let key = key.clone();
                let after = after.clone();
                window
                    .spawn(cx, async move |cx| {
                        let result = cx.background_spawn(async move { key.accept() }).await;
                        let _ = cx.update(|window, cx| after(result, window, cx));
                    })
                    .detach();
                true
            })
    });
}
