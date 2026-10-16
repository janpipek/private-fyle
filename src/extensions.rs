//! Static loading and validation of explicitly enabled extension packages.

/// File-read brokerage restricted to the selected native source.
pub mod broker;
mod loading;
/// Versioned manifest schema and provider declaration validation.
pub mod manifest;
/// Queue admission and active-slot lifecycle for supervised requests.
pub mod model;
/// Bounded wire schema and frame codec for supervised extension requests.
pub mod protocol;

pub use loading::{LoadReport, Package, PackageDiagnostic, load_enabled};
pub use manifest::PackageId;
