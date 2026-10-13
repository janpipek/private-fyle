//! Application menu bar with builders evaluated when a menu opens.
//!
//! The focus, switching, and popup lifecycle are adapted from GPUI Kit's
//! `app_menu_bar.rs` (https://github.com/longbridge/gpui-component), Copyright
//! 2024-2026 Longbridge, under the Apache License 2.0. This version accepts
//! menu builders so context-dependent contents can be loaded without closing
//! an open menu.

use std::rc::Rc;

use gpui_kit::base::StyledExt as _;
use gpui_kit::base::actions::{Cancel, SelectLeft, SelectRight};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::{Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

const CONTEXT: &str = "FyleMenuBar";

pub type MenuBuilder = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

pub struct MenuBar {
    menus: Vec<Entity<TopMenu>>,
    selected: Option<usize>,
    action_context: Option<FocusHandle>,
}

impl MenuBar {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.bind_keys([
            KeyBinding::new("escape", Cancel, Some(CONTEXT)),
            KeyBinding::new("left", SelectLeft, Some(CONTEXT)),
            KeyBinding::new("right", SelectRight, Some(CONTEXT)),
        ]);
        cx.new(|_| Self {
            menus: Vec::new(),
            selected: None,
            action_context: None,
        })
    }

    pub fn reload(&mut self, menus: Vec<(SharedString, MenuBuilder)>, cx: &mut Context<Self>) {
        let bar = cx.entity();
        self.menus = menus
            .into_iter()
            .enumerate()
            .map(|(index, (name, builder))| TopMenu::new(index, name, builder, bar.clone(), cx))
            .collect();
        self.selected = None;
        self.action_context = None;
        cx.notify();
    }

    fn move_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.selected else {
            return;
        };
        let next = if selected == 0 {
            self.menus.len().saturating_sub(1)
        } else {
            selected - 1
        };
        self.select(Some(next), window, cx);
    }

    fn move_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.selected else {
            return;
        };
        self.select(Some((selected + 1) % self.menus.len().max(1)), window, cx);
    }

    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.select(None, window, cx);
    }

    fn select(&mut self, selected: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected != selected
            && let Some(menu) = self.selected.and_then(|index| self.menus.get(index))
        {
            menu.update(cx, |menu, _| menu.close());
        }
        if self.selected.is_none() && selected.is_some() {
            self.action_context = window.focused(cx);
        } else if selected.is_none() {
            if let Some(handle) = &self.action_context {
                handle.focus(window, cx);
            }
            self.action_context = None;
        }
        self.selected = selected;
        cx.notify();
    }

    fn popup_dismissed(
        &mut self,
        index: usize,
        restore_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected != Some(index) {
            return;
        }
        if restore_focus && let Some(handle) = &self.action_context {
            handle.focus(window, cx);
        }
        self.action_context = None;
        self.selected = None;
        cx.notify();
    }

    fn trigger_closed(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected != Some(index) {
            return;
        }
        if let Some(handle) = &self.action_context {
            handle.focus(window, cx);
        }
        self.action_context = None;
        self.selected = None;
        cx.notify();
    }
}

impl Render for MenuBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let custom_titlebar = crate::window_chrome_view::uses_custom_titlebar(window, cx);
        div()
            .id("fyle-menu-bar")
            .role(Role::MenuBar)
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::move_left))
            .on_action(cx.listener(Self::move_right))
            .on_action(cx.listener(Self::cancel))
            .h_flex()
            .h_full()
            .when(!custom_titlebar, |bar| bar.w_full())
            .when(custom_titlebar, |bar| {
                // Keep menu clicks out of the custom title bar's window drag region.
                bar.occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            })
            .gap_x_1()
            .overflow_x_scroll()
            .children(self.menus.clone())
    }
}

struct TopMenu {
    bar: WeakEntity<MenuBar>,
    index: usize,
    name: SharedString,
    builder: MenuBuilder,
    popup: Option<Entity<PopupMenu>>,
    dismiss_subscription: Option<Subscription>,
}

impl TopMenu {
    fn new(
        index: usize,
        name: SharedString,
        builder: MenuBuilder,
        bar: Entity<MenuBar>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|_| Self {
            bar: bar.downgrade(),
            index,
            name,
            builder,
            popup: None,
            dismiss_subscription: None,
        })
    }

    fn is_open(&self, cx: &App) -> bool {
        self.bar
            .upgrade()
            .is_some_and(|bar| bar.read(cx).selected == Some(self.index))
    }

    fn popup(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<PopupMenu> {
        let action_context = self
            .bar
            .upgrade()
            .and_then(|bar| bar.read(cx).action_context.clone());
        let popup = match &self.popup {
            Some(popup) => popup.clone(),
            None => {
                let builder = self.builder.clone();
                let popup = PopupMenu::build(window, cx, move |menu, window, cx| {
                    let menu = builder(menu, window, cx);
                    match action_context {
                        Some(handle) => menu.action_context(handle),
                        None => menu,
                    }
                });
                self.dismiss_subscription = Some(cx.subscribe_in(
                    &popup,
                    window,
                    |this, _, _: &DismissEvent, window, cx| {
                        let restore_focus = this.popup.as_ref().is_some_and(|popup| {
                            popup.focus_handle(cx).contains_focused(window, cx)
                        });
                        this.dismiss_subscription.take();
                        this.popup.take();
                        if let Some(bar) = this.bar.upgrade() {
                            bar.update(cx, |bar, cx| {
                                bar.popup_dismissed(this.index, restore_focus, window, cx)
                            });
                        }
                    },
                ));
                self.popup = Some(popup.clone());
                popup
            }
        };
        popup.focus_handle(cx).focus(window, cx);
        popup
    }

    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(bar) = self.bar.upgrade() {
            if self.is_open(cx) {
                // We are already mutably borrowed while the trigger handles
                // this click, so close locally before updating the parent.
                self.close();
                bar.update(cx, |bar, cx| bar.trigger_closed(self.index, window, cx));
            } else {
                bar.update(cx, |bar, cx| bar.select(Some(self.index), window, cx));
            }
        }
    }

    fn hover(&mut self, hovered: &bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bar) = self.bar.upgrade() else {
            return;
        };
        if *hovered && bar.read(cx).selected.is_some() {
            bar.update(cx, |bar, cx| bar.select(Some(self.index), window, cx));
        }
    }

    fn close(&mut self) {
        self.dismiss_subscription.take();
        self.popup.take();
    }

    fn trigger_click(&mut self, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(event, ClickEvent::Mouse(_)) {
            self.toggle(window, cx);
        }
    }
}

impl Render for TopMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.is_open(cx);
        div()
            .id(self.index)
            .relative()
            .child(
                Button::new("menu")
                    .small()
                    .py_0p5()
                    .compact()
                    .ghost()
                    .label(self.name.clone())
                    .open(open)
                    .on_mouse_down(
                        MouseButton::Left,
                        window.listener_for(&cx.entity(), |this, _, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.toggle(window, cx);
                        }),
                    )
                    .on_click(cx.listener(Self::trigger_click)),
            )
            .on_hover(cx.listener(Self::hover))
            .when(open, |this| {
                this.child(deferred(
                    anchored()
                        .anchor(Anchor::TopLeft)
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            div()
                                .size_full()
                                .occlude()
                                .top_1()
                                .child(self.popup(window, cx)),
                        ),
                ))
            })
    }
}
