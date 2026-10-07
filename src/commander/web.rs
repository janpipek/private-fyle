//! "Open in Browser" for remote items: the provider builds the private web
//! address in the background, then the system opens it.

use std::sync::atomic::AtomicBool;

use gpui_kit::*;

use super::Commander;
use super::keys::OpenInBrowser;
use crate::menus::Cmd;
use crate::remote::{self, Control};

impl Commander {
    /// Opens the item under the cursor, or the current folder when the
    /// cursor is on "..", in the service's web view (which needs the user's
    /// own login). Errors go to the status line.
    pub fn open_in_browser(&mut self, _: &OpenInBrowser, _: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_cmd(Cmd::OpenInBrowser, cx) {
            return;
        }
        let folder = self.folders[self.active].read(cx);
        let location = match folder.current() {
            Some(row) if !row.is_parent => row.location.clone(),
            _ => match folder.location() {
                Some(location) => location.clone(),
                None => {
                    self.status = "Drives: not a remote folder".into();
                    cx.notify();
                    return;
                }
            },
        };
        let path = location.panel_path();
        let (provider, location) = match self.resolve_remote(&location) {
            Ok(found) => found,
            Err(message) => {
                self.status = message.into();
                cx.notify();
                return;
            }
        };
        self.status = format!("{}: looking up the web address…", path.display()).into();
        cx.notify();
        let at_root = location.name().is_none();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let cancel = AtomicBool::new(false);
                    provider.web_url(&location, &Control::new(&cancel))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(url) => match open::that_detached(&url) {
                        Ok(()) if !at_root && remote::is_start_page(&url) => {
                            format!("Opened {url}: the exact item could not be addressed")
                        }
                        Ok(()) => format!("Opened {url}"),
                        Err(e) => format!("{url}: {e}"),
                    },
                    Err(e) => format!("{}: {e}", path.display()),
                }
                .into();
                cx.notify();
            });
        })
        .detach();
    }
}
