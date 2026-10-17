//! Static loading and validation of explicitly enabled extension packages.

/// File-read brokerage restricted to the selected native source.
pub mod broker;
/// Builds the application provider catalog from explicitly enabled packages.
pub mod catalog;
mod loading;
/// Versioned manifest schema and provider declaration validation.
pub mod manifest;
/// Queue admission and active-slot lifecycle for supervised requests.
pub mod model;
/// Adapts validated viewer contributions to native preview requests.
pub mod preview;
/// Adapts declared Properties contributions to the application inspector.
pub mod properties;
/// Bounded wire schema and frame codec for supervised extension requests.
pub mod protocol;
/// Typed result values validated against the selected manifest contribution.
pub mod results;
/// Supervised single-request Lua worker and bounded process transport.
pub mod supervisor;
/// Restricted Lua callback execution used only by the reserved helper mode.
pub mod worker;

pub use loading::{LoadReport, Package, PackageDiagnostic, load_enabled};
pub use manifest::PackageId;
pub use supervisor::{ColumnWork, ContributionTicket, ContributionWork, Supervisor, Ticket};

/// Dispatches the reserved helper invocation before application startup touches settings or GPUI.
pub fn run_worker_if_requested() {
    use std::ffi::OsStr;
    if std::env::args_os().nth(1).as_deref() != Some(OsStr::new("--fyle-extension-worker-v1")) {
        return;
    }
    let result = {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut input = stdin.lock();
        let mut output = stdout.lock();
        worker::run(&mut input, &mut output)
    };
    match result {
        Ok(()) => std::process::exit(0),
        Err(error) => {
            eprintln!("extension worker protocol failed: {error}");
            std::process::exit(2);
        }
    }
}
