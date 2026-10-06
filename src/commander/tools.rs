//! Tools: the context, the Tools menu, parameter prompts and the tool log.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, IndexPath, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Commander;
use super::keys::ToolsMenu;
use super::menus::command_item;
use crate::menus::{self, Cmd};
use crate::tools::{
    self, Action as ToolAction, Offer, ParamKind, Registry, Target, ToolContext, Values,
};
use crate::tools_view::{MenuDelegate, PickDelegate};

pub fn tool_menu_footer(mut menu: PopupMenu, state: &menus::State) -> PopupMenu {
    menu = menu.separator();
    for cmd in [Cmd::ToolsMenu, Cmd::FindFiles] {
        menu = menu.item(command_item(cmd, state));
    }
    menu.separator().item(command_item(Cmd::CancelJobs, state))
}

/// Built-in tools, then the user's `tools.toml`, then their Lua plugins.
pub fn load_tools() -> (Registry, Option<String>) {
    let mut registry = Registry::with_defaults();
    let Some(config) = dirs::config_dir().map(|d| d.join("fyle")) else {
        return (registry, None);
    };
    let path = config.join("tools.toml");
    let mut errors: Vec<String> = match std::fs::read_to_string(&path) {
        Ok(text) => registry
            .load_toml(&text)
            .err()
            .map(|e| format!("{}: {e}", path.display()))
            .into_iter()
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => vec![format!("{}: {e}", path.display())],
    };
    errors.extend(registry.load_plugins(&config.join("plugins")));
    (registry, (!errors.is_empty()).then(|| errors.join("; ")))
}

/// Shows a command's output in a scrollable dialog.
pub fn show_log(title: &str, log: String, window: &mut Window, cx: &mut App) {
    let title = format!("{title}: output");
    let lines: Rc<Vec<String>> = Rc::new(log.lines().map(String::from).collect());
    window.open_dialog(cx, move |dialog, _, cx| {
        let mono = cx.theme().mono_font_family.clone();
        dialog.title(title.clone()).w(px(760.)).child(
            div()
                .id("log")
                .h(px(420.))
                .overflow_y_scroll()
                .font_family(mono)
                .text_xs()
                .children(
                    lines
                        .iter()
                        .map(|l| div().whitespace_nowrap().child(l.clone())),
                ),
        )
    });
}

impl Commander {
    /// What the tools see: the active panel and the other panel's directory.
    pub fn tool_context(&self, cx: &App) -> ToolContext {
        let folder = self.folders[self.active].read(cx);
        let other = self.folders[1 - self.active].read(cx).path();
        ToolContext {
            dir: folder.path().to_path_buf(),
            targets: folder
                .targets()
                .into_iter()
                .map(|path| {
                    let is_dir = path.is_dir();
                    Target::new(path, is_dir)
                })
                .collect(),
            other_dir: (!other.as_os_str().is_empty()).then(|| other.to_path_buf()),
        }
    }

    /// F2: tools applicable to the selection / directory, grouped.
    pub fn tools_menu(&mut self, _: &ToolsMenu, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_in_archive(cx) || self.refuse_on_remote(cx) {
            return;
        }
        let ctx = self.tool_context(cx);
        let facts = tools::detect::detect(&ctx);
        let offers = self.tools.offers(&ctx, &facts, &self.programs);
        if offers.is_empty() {
            self.status = "No tools for this selection".into();
            cx.notify();
            return;
        }
        let this = cx.entity().downgrade();
        let delegate = MenuDelegate::new(offers, move |offer: Offer, window, cx| {
            window.close_dialog(cx);
            let (ctx, facts) = (ctx.clone(), facts.clone());
            let _ = this.update(cx, |this, cx| {
                this.ask_params(offer, ctx, facts, window, cx)
            });
        });
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(0)), window, cx)
        });
        let dialog_list = list.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title("Tools").w(px(480.)).child(
                div()
                    .h(px(400.))
                    .child(List::new(&dialog_list).search_placeholder("Filter tools…")),
            )
        });
        list.update(cx, |list, cx| list.focus(window, cx));
    }

    /// Asks for the tool's parameters (if any), then plans it.
    pub fn ask_params(
        &mut self,
        offer: Offer,
        ctx: ToolContext,
        facts: tools::Facts,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let params = self
            .tools
            .tool(&offer.id)
            .map(|t| t.params(&ctx))
            .unwrap_or_default();
        if params.is_empty() {
            self.plan_tool(offer, ctx, facts, Values::new(), window, cx);
            return;
        }
        enum Field {
            Text(Entity<InputState>),
            Choice(Vec<String>, Rc<RefCell<String>>),
        }
        let fields: Rc<Vec<(String, String, Field)>> = Rc::new(
            params
                .into_iter()
                .map(|p| {
                    let default = p.default_value();
                    let field =
                        match p.kind {
                            ParamKind::Text { .. } => Field::Text(cx.new(|cx| {
                                InputState::new(window, cx).default_value(default.clone())
                            })),
                            ParamKind::Choice(options) => {
                                Field::Choice(options, Rc::new(RefCell::new(default)))
                            }
                        };
                    (p.key, p.label, field)
                })
                .collect(),
        );
        let this = cx.entity().downgrade();
        let first_input = fields.iter().find_map(|(_, _, f)| match f {
            Field::Text(input) => Some(input.clone()),
            Field::Choice(..) => None,
        });
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let form = v_flex()
                .gap_3()
                .w_full()
                .children(fields.iter().enumerate().map(|(i, (_, label, field))| {
                    let control = match field {
                        Field::Text(input) => Input::new(input).into_any_element(),
                        Field::Choice(options, value) => h_flex()
                            .flex_wrap()
                            .gap_1()
                            .children(options.iter().enumerate().map(|(j, option)| {
                                let (value, option) = (value.clone(), option.clone());
                                let selected = *value.borrow() == option;
                                Button::new(("choice", i * 100 + j))
                                    .label(option.clone())
                                    .small()
                                    .when(selected, |b| b.primary())
                                    .when(!selected, |b| b.outline())
                                    .on_click(move |_, window, _| {
                                        *value.borrow_mut() = option.clone();
                                        window.refresh();
                                    })
                            }))
                            .into_any_element(),
                    };
                    v_flex().gap_1().child(label.clone()).child(control)
                }));
            let (this, fields, offer, ctx, facts) = (
                this.clone(),
                fields.clone(),
                offer.clone(),
                ctx.clone(),
                facts.clone(),
            );
            dialog
                .title(offer.label.clone())
                .description(form)
                .ok_text("Continue")
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let values: Values = fields
                        .iter()
                        .map(|(key, _, field)| {
                            let value = match field {
                                Field::Text(input) => input.read(cx).value().to_string(),
                                Field::Choice(_, value) => value.borrow().clone(),
                            };
                            (key.clone(), value)
                        })
                        .collect();
                    let (offer, ctx, facts) = (offer.clone(), ctx.clone(), facts.clone());
                    let _ = this.update(cx, |this, cx| {
                        this.plan_tool(offer, ctx, facts, values, window, cx)
                    });
                    true
                })
        });
        if let Some(input) = first_input {
            input.update(cx, |input, cx| input.focus(window, cx));
        }
    }

    /// Plans in the background (a tool may run quick probes).
    pub fn plan_tool(
        &mut self,
        offer: Offer,
        ctx: ToolContext,
        facts: tools::Facts,
        values: Values,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status = format!("{}…", offer.label.trim_end_matches('…')).into();
        cx.notify();
        let (registry, programs) = (self.tools.clone(), self.programs.clone());
        self._tool_task = cx.spawn_in(window, async move |this, cx| {
            let id = offer.id.clone();
            let result = cx
                .background_spawn(
                    async move { registry.plan(&id, &ctx, &facts, &programs, &values) },
                )
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.handle_tool_action(offer.label, result, window, cx)
            });
        });
    }

    /// Resolves picks, confirms anything that changes files, then runs.
    pub fn handle_tool_action(
        &mut self,
        label: String,
        result: Result<ToolAction, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = match result {
            Ok(action) => action,
            Err(e) => {
                self.status = format!("{label}: {e}").into();
                cx.notify();
                return;
            }
        };
        let this = cx.entity().downgrade();
        match action {
            ToolAction::Pick { title, items } => {
                let labels = items
                    .iter()
                    .map(|i| match &i.detail {
                        Some(detail) => format!("{} — {detail}", i.label),
                        None => i.label.clone(),
                    })
                    .collect();
                let items = Rc::new(RefCell::new(items));
                let delegate = PickDelegate::new(labels, move |index, window, cx| {
                    window.close_dialog(cx);
                    let mut items = items.borrow_mut();
                    if index < items.len() {
                        let item = items.swap_remove(index);
                        let label = item.label.clone();
                        let _ = this.update(cx, |this, cx| {
                            this.handle_tool_action(label, Ok(item.action), window, cx)
                        });
                    }
                });
                let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
                list.update(cx, |list, cx| {
                    list.set_selected_index(Some(IndexPath::new(0)), window, cx)
                });
                let dialog_list = list.clone();
                window.open_dialog(cx, move |dialog, _, _| {
                    dialog
                        .title(title.clone())
                        .w(px(420.))
                        .child(div().h(px(360.)).child(List::new(&dialog_list)))
                });
                list.update(cx, |list, cx| list.focus(window, cx));
            }
            action if action.is_harmless() => self.run_tool_action(label, action, window, cx),
            action => {
                let lines = action.describe();
                let mut shown: Vec<String> = lines.iter().take(12).cloned().collect();
                if lines.len() > shown.len() {
                    shown.push(format!("… and {} more", lines.len() - shown.len()));
                }
                let pending = Rc::new(RefCell::new(Some(action)));
                window.open_alert_dialog(cx, move |dialog, _, cx| {
                    let (this, pending, label) = (this.clone(), pending.clone(), label.clone());
                    let mono = cx.theme().mono_font_family.clone();
                    dialog
                        .title(label.clone())
                        .description(
                            v_flex()
                                .gap_1()
                                .font_family(mono)
                                .text_xs()
                                .children(shown.iter().map(|l| div().child(l.clone()))),
                        )
                        .ok_text("Run")
                        .show_cancel(true)
                        .on_ok(move |_, window, cx| {
                            if let Some(action) = pending.borrow_mut().take() {
                                let label = label.clone();
                                let _ = this.update(cx, |this, cx| {
                                    this.run_tool_action(label, action, window, cx)
                                });
                            }
                            true
                        })
                });
            }
        }
    }

    pub fn run_tool_action(
        &mut self,
        label: String,
        action: ToolAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status = format!("Running {label}…").into();
        cx.notify();
        self._tool_task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { tools::exec::run(action) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(report) => {
                        this.status = format!("{label}: {}", report.summary).into();
                        if report.refresh {
                            this.reload_all(cx);
                        }
                        if !report.log.trim().is_empty() {
                            show_log(&label, report.log, window, cx);
                        }
                    }
                    Err(e) => this.status = format!("{label}: {e}").into(),
                }
                cx.notify();
            });
        });
    }

    /// Snapshot the active panel once, then discover tools away from the UI thread.
    pub fn discover_tools_for_menu(
        &mut self,
        popup: WeakEntity<PopupMenu>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.tools_menu_generation = self.tools_menu_generation.wrapping_add(1);
        let generation = self.tools_menu_generation;
        if self.folders[self.active].read(cx).is_remote() {
            if let Some(popup) = popup.upgrade() {
                popup.update(cx, |popup, cx| {
                    popup.rebuild(window, cx, |menu, _, _| {
                        menu.label("Not available on remote folders")
                    })
                });
            }
            return;
        }
        if self.folders[self.active].read(cx).archive().is_some() {
            if let Some(popup) = popup.upgrade() {
                popup.update(cx, |popup, cx| {
                    popup.rebuild(window, cx, |menu, _, _| {
                        menu.label("Not available inside an archive")
                    })
                });
            }
            return;
        }
        let context = self.tool_context(cx);
        let detection_context = context.clone();
        let (registry, programs) = (self.tools.clone(), self.programs.clone());
        self._tools_menu_task = cx.spawn_in(window, async move |this, cx| {
            let (facts, groups) = cx
                .background_spawn(async move {
                    let facts = tools::detect::detect(&detection_context);
                    let offers = registry.offers(&detection_context, &facts, &programs);
                    (facts, tools::group_offers(offers))
                })
                .await;
            let _ =
                this.update_in(cx, |this, window, cx| {
                    if this.tools_menu_generation != generation {
                        return;
                    }
                    let Some(popup) = popup.upgrade() else { return };
                    let commander = cx.weak_entity();
                    let state = this.menu_state(cx);
                    popup.update(cx, |popup, cx| {
                        popup.rebuild(window, cx, |mut menu, _, _| {
                            if groups.is_empty() {
                                menu = menu.label("No tools for this selection");
                            } else {
                                for group in &groups {
                                    menu = menu.label(group.label.clone());
                                    for offer in &group.offers {
                                        let selected_offer = offer.clone();
                                        let context = context.clone();
                                        let facts = facts.clone();
                                        let commander = commander.clone();
                                        let missing = offer.missing.clone();
                                        let label = offer.label.clone();
                                        let item = PopupMenuItem::element(move |_, cx| {
                                            h_flex()
                                                .w_full()
                                                .justify_between()
                                                .gap_4()
                                                .child(label.clone())
                                                .when(!missing.is_empty(), |row| {
                                                    row.child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(format!(
                                                                "needs {}",
                                                                missing.join(", ")
                                                            )),
                                                    )
                                                })
                                        })
                                        .disabled(!offer.missing.is_empty())
                                        .on_click(move |_, window, cx| {
                                            let offer = selected_offer.clone();
                                            let context = context.clone();
                                            let facts = facts.clone();
                                            let _ = commander.update(cx, |this, cx| {
                                                this.ask_params(offer, context, facts, window, cx)
                                            });
                                        });
                                        menu = menu.item(item);
                                    }
                                }
                            }
                            tool_menu_footer(menu, &state)
                        });
                    });
                });
        });
    }
}
