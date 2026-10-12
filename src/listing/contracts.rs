//! Plain request and result values shared by the listing service and panel model.

use std::collections::HashSet;
use std::io;

use crate::entry::{Entry, Sort};
use crate::location::{Location, PanelTarget};

/// Immutable options captured when a panel listing is requested.
#[derive(Clone)]
pub struct ListingRequest {
    /// Directory or platform places target to read.
    pub target: PanelTarget,
    /// Whether expanded native directories should be included recursively.
    pub tree: bool,
    /// Typed locations expanded in tree mode.
    pub expanded: HashSet<Location>,
    /// Sibling ordering used when flattening a tree listing.
    pub sort: Sort,
    /// Return destination when browsing up from a remote root.
    pub remote_origin: Option<Location>,
    /// Whether a failed local target may be replaced by its readable ancestor.
    pub allow_ancestor_fallback: bool,
    /// Existing search hits to retain only when their native files still exist.
    pub search_hits: Option<Vec<ListingRow>>,
    /// Provider-backed reads must run despite a cached remote listing.
    pub force_remote_refresh: bool,
}

/// One row in a completed snapshot, with identity independent of display path.
#[derive(Clone, Debug)]
pub struct ListingRow {
    /// Display and metadata fields for the row.
    pub entry: Entry,
    /// Stable local, remote, or archive identity for navigation and marks.
    pub location: Location,
    /// Nesting depth, zero for ordinary directory listings.
    pub depth: usize,
    /// Matching line information retained by a content search.
    pub hit_line: Option<(u64, String)>,
}

/// Completed rows and non-fatal provider warnings for the actual target.
#[derive(Clone, Debug)]
pub struct ListingSnapshot {
    /// The target that was successfully listed.
    pub target: PanelTarget,
    /// Rows in display order, including a parent row where one exists.
    pub rows: Vec<ListingRow>,
    /// Provider warnings such as skipped duplicate or unsafe names.
    pub warnings: Vec<String>,
    /// Search snapshots retain their search title in the panel model.
    pub search_results: bool,
    /// Return destination when the target is a remote root.
    pub remote_origin: Option<Location>,
}

/// Successful listing after best-effort local ancestor recovery.
#[derive(Debug)]
pub struct ListingOutcome {
    /// The originally requested target.
    pub requested: PanelTarget,
    /// The first error encountered before recovery, if any.
    pub first_error: Option<io::Error>,
    /// Listing of the request or its nearest readable local ancestor.
    pub snapshot: ListingSnapshot,
}
