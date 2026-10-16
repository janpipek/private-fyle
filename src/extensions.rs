//! Static loading and validation of explicitly enabled extension packages.

mod loading;
/// Versioned manifest schema and provider declaration validation.
pub mod manifest;

pub use loading::{LoadReport, Package, PackageDiagnostic, load_enabled};
pub use manifest::PackageId;
