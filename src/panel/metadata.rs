//! GPUI scheduling of bounded metadata work shared by list and icon panel views.

/// EXIF projection from semantic tags to stable metadata-column values.
pub mod exif;
/// UI-independent request policy, contracts and cache.
pub mod model;
/// Blocking optional metadata reader.
pub mod reader;

use std::path::Path;

use gpui_kit::{AppContext as _, Context};

use crate::links::LinkState;
use crate::location::Location;

use model::MetadataQueue;
/// Metadata fields, display values and visible-column selection shared by panel views.
pub use model::{Field, MetadataValues, fields_for_row};

/// Cache and background workers shared by the list and icon views of a panel.
pub struct MetadataState {
    queue: MetadataQueue,
}

impl MetadataState {
    /// Creates an empty metadata cache for one panel.
    pub fn new() -> Self {
        Self {
            queue: MetadataQueue::default(),
        }
    }

    /// Returns completed values for a path in the current listing generation.
    pub fn get(&self, path: &Path, generation: u64) -> Option<&MetadataValues> {
        self.queue.get(path, generation)
    }

    /// Queues missing local metadata fields and starts bounded background work.
    pub fn request(
        &mut self,
        location: Location,
        generation: u64,
        fields: &[Field],
        uid: Option<u32>,
        link: LinkState,
        cx: &mut Context<Self>,
    ) {
        self.queue.request(&location, generation, fields, uid, link);
        self.start(cx);
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        while let Some(request) = self.queue.take_next() {
            let worker_request = request.clone();
            let state = cx.weak_entity();
            cx.spawn(async move |_, cx| {
                let values = cx
                    .background_spawn(async move { reader::read_metadata(&worker_request) })
                    .await;
                let _ = state.update(cx, |state, cx| {
                    if state.queue.complete(request, values) {
                        cx.notify();
                    }
                    state.start(cx);
                });
            })
            .detach();
        }
    }
}
