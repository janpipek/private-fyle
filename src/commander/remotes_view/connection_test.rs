//! Background remote connection testing and explicit SSH trust retry.

use gpui_kit::*;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::RemoteManager;
use crate::remote::{Control, Location, Provider};

impl RemoteManager {
    /// Tests the current draft and offers trust confirmation for an unknown SSH key.
    pub fn test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_test();
        self.busy = true;
        self.message = Some("Testing connection…".into());
        let config = match self.draft(cx).validate() {
            Ok(config) => config,
            Err(error) => {
                self.busy = false;
                self.message = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let entity = cx.entity().downgrade();
        self.test_generation = self.test_generation.wrapping_add(1);
        let generation = self.test_generation;
        let cancel = Arc::new(AtomicBool::new(false));
        self.test_cancel = Some(cancel.clone());
        let tested_config = config.clone();
        self.test_task = cx.spawn_in(window, async move |_, cx| {
            let result = cx
                .background_spawn(async move {
                    let provider = crate::remote::remotefs::RemoteFsProvider::new(config.clone());
                    let root = Location::root(&config.name)?;
                    provider
                        .list(&root, &Control::new(&cancel))
                        .map(|listing| listing.entries.len())
                })
                .await;
            let _ = entity.update_in(cx, |this, _window, cx| {
                #[cfg(feature = "sftp")]
                let window = _window;
                if this.test_generation != generation {
                    return;
                }
                this.busy = false;
                this.test_cancel = None;
                if !this
                    .draft(cx)
                    .validate()
                    .is_ok_and(|current| current == tested_config)
                {
                    cx.notify();
                    return;
                }
                #[cfg(feature = "sftp")]
                if let Err(error) = &result
                    && let Some(key) = crate::remote::hostkey::UnknownKey::from_error(error)
                {
                    let entity = cx.entity().downgrade();
                    let tested_config = tested_config.clone();
                    super::super::hostkey_view::confirm(
                        key,
                        window,
                        cx,
                        move |result, window, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                if this.test_generation != generation
                                    || !this
                                        .draft(cx)
                                        .validate()
                                        .is_ok_and(|current| current == tested_config)
                                {
                                    return;
                                }
                                match result {
                                    Ok(()) => this.test(window, cx),
                                    Err(error) => {
                                        this.message =
                                            Some(format!("Cannot save SSH host key: {error}"));
                                        cx.notify();
                                    }
                                }
                            });
                        },
                    );
                }
                this.message = Some(match result {
                    Ok(count) => format!("Connection works; {count} items at the root"),
                    Err(error) => format!("{}: connection failed: {error}", tested_config.name),
                });
                cx.notify();
            });
        });
    }
}
