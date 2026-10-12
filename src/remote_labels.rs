//! The service names of the configured remotes, shared with the views that
//! label breadcrumbs, so they need no access to the provider registry.

use std::collections::HashMap;

use gpui_kit::*;

/// Remote name to service label (e.g. `gdrive` to "Google Drive"), set by
/// the commander whenever remotes are discovered.
#[derive(Default)]
pub struct RemoteLabels(pub HashMap<String, String>);

impl Global for RemoteLabels {}

/// The label of the remote a panel path belongs to, if it is remote and known.
pub fn label_of(path: &std::path::Path, cx: &App) -> Option<String> {
    let name = crate::location::RemoteLocation::parse(path)
        .ok()??
        .remote()
        .to_owned();
    cx.try_global::<RemoteLabels>()?.0.get(&name).cloned()
}
