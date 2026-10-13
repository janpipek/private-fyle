//! Window decoration options and themed title bars.

#[cfg(any(windows, target_os = "linux"))]
use gpui_kit::component::{ActiveTheme as _, TitleBar, h_flex};
use gpui_kit::*;

/// Custom title bar mode fixed for all windows until the next application start.
#[derive(Clone, Copy)]
pub struct StartupCustomTitlebar(
    /// Whether new windows use the custom title bar.
    pub bool,
);

impl Global for StartupCustomTitlebar {}

/// Returns the custom title bar mode selected when the application started.
#[cfg(any(windows, target_os = "linux"))]
pub fn startup_custom_titlebar(cx: &App) -> bool {
    cx.global::<StartupCustomTitlebar>().0
}

/// Reports whether the platform selected the client-decorated title bar.
#[cfg(windows)]
pub fn uses_custom_titlebar(_: &Window, cx: &App) -> bool {
    startup_custom_titlebar(cx)
}

/// Reports whether Linux is using client decorations, including a compositor fallback.
#[cfg(target_os = "linux")]
pub fn uses_custom_titlebar(window: &Window, _cx: &App) -> bool {
    linux_uses_custom_titlebar(window.window_decorations())
}

#[cfg(target_os = "linux")]
fn linux_uses_custom_titlebar(decorations: Decorations) -> bool {
    matches!(decorations, Decorations::Client { .. })
}

/// Reports that other platforms retain their native title bar.
#[cfg(not(any(windows, target_os = "linux")))]
pub fn uses_custom_titlebar(_: &Window, _: &App) -> bool {
    false
}

/// Selects client decorations for themed bars on Windows and Linux.
pub fn window_options(title: String, cx: &App) -> WindowOptions {
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = cx;
    #[cfg(windows)]
    let mut options = if startup_custom_titlebar(cx) {
        TitleBar::window_options()
    } else {
        WindowOptions::default()
    };
    #[cfg(target_os = "linux")]
    let mut options = if startup_custom_titlebar(cx) {
        WindowOptions {
            window_decorations: Some(WindowDecorations::Client),
            ..TitleBar::window_options()
        }
    } else {
        WindowOptions::default()
    };
    #[cfg(not(any(windows, target_os = "linux")))]
    let mut options = WindowOptions::default();
    let titlebar = options
        .titlebar
        .get_or_insert_with(TitlebarOptions::default);
    titlebar.title = Some(title.into());
    options
}

/// Combines the application title, menus and window controls in one row.
#[cfg(any(windows, target_os = "linux"))]
pub fn menu_title_bar(menu: Entity<crate::menu_bar::MenuBar>, cx: &App) -> impl IntoElement {
    TitleBar::new()
        .bg(cx.theme().title_bar)
        .text_color(cx.theme().foreground)
        .child(div().flex_none().mr_3().text_sm().child("Fyle"))
        .child(
            h_flex()
                .h_full()
                // A zero basis keeps long menus from widening GPUI's drag region.
                .w_0()
                .flex_1()
                .min_w_0()
                .child(menu),
        )
        .child(div().flex_none().w(px(48.)).h_full())
}

/// Gives the editor a draggable title and window controls beside Save.
#[cfg(any(windows, target_os = "linux"))]
pub fn editor_title_bar(
    title: String,
    save: impl IntoElement,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let title_bar = TitleBar::new()
        .bg(cx.theme().title_bar)
        .text_color(cx.theme().foreground)
        .child(
            div()
                // GPUI's drag region has a content-based minimum width otherwise.
                .w_0()
                .flex_1()
                .min_w_0()
                .text_sm()
                .truncate()
                .child(title),
        )
        .child(
            h_flex()
                .flex_none()
                .h_full()
                .px_2()
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(save),
        );
    #[cfg(target_os = "linux")]
    let title_bar = title_bar.on_close_window(on_close);
    #[cfg(windows)]
    let title_bar = {
        let _ = on_close;
        title_bar
    };
    title_bar
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use gpui_kit::{Decorations, Tiling};

    use super::linux_uses_custom_titlebar;

    #[test]
    fn client_decorations_use_the_custom_title_bar_even_when_forced() {
        assert!(linux_uses_custom_titlebar(Decorations::Client {
            tiling: Tiling::default(),
        }));
    }

    #[test]
    fn server_decorations_keep_the_native_title_bar() {
        assert!(!linux_uses_custom_titlebar(Decorations::Server));
    }
}
