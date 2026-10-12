//! Window decoration options and themed Windows title bars.

#[cfg(windows)]
use gpui_kit::component::{ActiveTheme as _, TitleBar, h_flex};
use gpui_kit::*;

/// Keeps native decorations outside Windows and enables the themed Windows bar.
pub fn window_options(title: String) -> WindowOptions {
    #[cfg(windows)]
    let mut options = TitleBar::window_options();
    #[cfg(not(windows))]
    let mut options = WindowOptions::default();
    let titlebar = options
        .titlebar
        .get_or_insert_with(TitlebarOptions::default);
    titlebar.title = Some(title.into());
    options
}

/// Combines the application title, menus and native Windows controls in one row.
#[cfg(windows)]
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

/// Gives the editor a draggable title and native Windows controls beside Save.
#[cfg(windows)]
pub fn editor_title_bar(title: String, save: impl IntoElement, cx: &App) -> impl IntoElement {
    TitleBar::new()
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
        )
}
