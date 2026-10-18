//! Native bounded reader seam for provider-defined table columns.

use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::data_providers::{DemandLimits, ResultState, Value};
use crate::location::Location;

/// Captured row data passed to a native provider column reader in a worker.
#[derive(Clone)]
pub struct ProviderColumnRequest {
    /// Original typed local source identity.
    pub location: Location,
    /// Name captured from the listing.
    pub name: String,
    /// Whether the row is a directory.
    pub is_dir: bool,
    /// The contribution's declared read bounds.
    pub demand: DemandLimits,
    /// Cancellation for the listing generation.
    pub cancel: Arc<AtomicBool>,
}

/// Host-owned native acquisition callback for one column contribution.
///
/// Returns a typed value state, including partial completeness where known.
/// The host callback enforces its declared byte and item limits; the catalog
/// verifies ready values against the provider's field declaration.
pub type NativeColumnReader =
    Arc<dyn Fn(&ProviderColumnRequest) -> io::Result<ResultState<Value>> + Send + Sync + 'static>;
