//! Draws symbolic links: the target's file icon with a small badge in its
//! corner. The badge differs in shape, not only colour: a curved arrow for a
//! working link, a crossed circle for a broken one. Views for other layouts
//! (an icon grid) can wrap their icon with [`badged`] too.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, Size};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::links::LinkState;

/// The badge glyph for a link state; `None` for ordinary entries.
pub fn badge_glyph(link: &LinkState) -> Option<Lucide> {
    match link {
        LinkState::NotLink => None,
        LinkState::Link { .. } => Some(Lucide::Redo2),
        LinkState::Broken { .. } => Some(Lucide::Ban),
    }
}

/// `icon` with the link badge at its bottom-right corner (unchanged for
/// ordinary entries).
pub fn badged(icon: Icon, link: &LinkState, cx: &App) -> Div {
    badged_sized(icon, link, Size::XSmall, cx)
}

/// [`badged`] with a badge of `size`, for large icons.
pub fn badged_sized(icon: Icon, link: &LinkState, size: Size, cx: &App) -> Div {
    let theme = cx.theme();
    let color = if link.is_broken() {
        theme.danger
    } else {
        theme.foreground
    };
    div()
        .relative()
        .flex_none()
        .child(icon)
        .when_some(badge_glyph(link), |d, glyph| {
            d.child(
                div()
                    .absolute()
                    .right(px(-4.))
                    .bottom(px(-3.))
                    .rounded_full()
                    .bg(theme.background)
                    .text_color(color)
                    .child(Icon::new(glyph).with_size(size)),
            )
        })
}
