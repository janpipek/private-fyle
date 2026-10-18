//! Pure rules, scan outcomes, and bounded category values for column filters.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::columns::ColumnId;
use crate::location::Location;

/// A value visible in the categorical filter chooser.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CategoryValue {
    /// A nonempty displayed value.
    Text(String),
    /// A successfully read value that the column displays as blank.
    Blank,
}

impl CategoryValue {
    /// Stable label used in the filter chooser.
    pub fn label(&self) -> &str {
        match self {
            Self::Text(value) => value,
            Self::Blank => "Blank",
        }
    }
}

/// One rule's matching behavior.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MatchMode {
    /// Case-insensitive substring of the displayed value.
    Contains(String),
    /// Exact category choices; selected values are ORed together.
    AnyOf(BTreeSet<CategoryValue>),
}

/// Editing mode for a column-filter draft.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DraftMode {
    /// Match a case-insensitive substring.
    Text,
    /// Match selected exact categories.
    #[default]
    Values,
}

/// UI-independent text and category choices before the user applies them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterDraft {
    /// Whether the input is a substring or a category search query.
    pub mode: DraftMode,
    /// Text rule or category search query, depending on the selected mode.
    pub query: String,
    /// Exact choices selected in Values mode.
    pub selected: BTreeSet<CategoryValue>,
}

impl FilterDraft {
    /// Converts the draft into an applied rule, or `None` when it is empty.
    pub fn rule(&self, column: ColumnId) -> Option<ColumnRule> {
        let mode = match self.mode {
            DraftMode::Text if !self.query.is_empty() => MatchMode::Contains(self.query.clone()),
            DraftMode::Text => return None,
            DraftMode::Values if !self.selected.is_empty() => {
                MatchMode::AnyOf(self.selected.clone())
            }
            DraftMode::Values => return None,
        };
        Some(ColumnRule { column, mode })
    }
}

/// A filter rule attached to one persisted column ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnRule {
    /// Column whose displayed values are matched.
    pub column: ColumnId,
    /// Matching mode for this column.
    pub mode: MatchMode,
}

/// The result of acquiring one column value for a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueOutcome {
    /// A complete or partial displayed value.
    Ready {
        /// Text projected by the same formatter used by the table cell.
        text: String,
        /// Whether the displayed text is complete, bounded, or a lower bound.
        completeness: ValueCompleteness,
    },
    /// The value was read successfully and the cell is blank.
    Blank,
    /// The read has not completed yet.
    Pending,
    /// The read failed; the failure remains distinct from blank.
    Failed(String),
    /// This source or row cannot provide this value.
    Unavailable(String),
}

/// Completeness retained with a value used for matching and facets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueCompleteness {
    /// The source reports a complete value.
    Complete,
    /// The source reports a bounded partial value.
    Partial(String),
    /// The source reports a lower bound.
    LowerBound,
}

/// The row's outcome after applying every active column rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowOutcome {
    /// All active column rules matched.
    Match,
    /// At least one ready value did not match.
    NoMatch,
    /// No rule rejected the row, but a required value is still being read.
    Pending,
    /// A required source failed.
    Failed,
    /// A required source does not exist for this row.
    Unavailable,
}

impl ColumnRule {
    /// Matches one acquired value using the rule's declared semantics.
    pub fn matches(&self, outcome: &ValueOutcome) -> bool {
        match (&self.mode, outcome) {
            (MatchMode::Contains(needle), ValueOutcome::Ready { text, .. }) => {
                !needle.is_empty() && text.to_lowercase().contains(&needle.to_lowercase())
            }
            (MatchMode::AnyOf(values), ValueOutcome::Ready { text, .. }) => {
                values.contains(&CategoryValue::Text(text.clone()))
            }
            (MatchMode::AnyOf(values), ValueOutcome::Blank) => {
                values.contains(&CategoryValue::Blank)
            }
            _ => false,
        }
    }
}

/// Aggregated status of a scan, including rows withheld while values are pending.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterProgress {
    /// Number of rows captured from the current listing.
    pub total: usize,
    /// Number of rows whose active column values have all completed.
    pub completed: usize,
    /// Rows currently confirmed to match.
    pub matched: usize,
    /// Rows waiting on one or more values.
    pub pending: usize,
    /// Rows with failed reads.
    pub failed: usize,
    /// Rows with unavailable sources or values.
    pub unavailable: usize,
}

/// Bounded acquisition diagnostics for categories in one column.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FacetDiagnostics {
    /// Values still being read or awaiting their source.
    pub pending: usize,
    /// Reads that failed.
    pub failed: usize,
    /// Rows without a supported source or provider.
    pub unavailable: usize,
    /// One representative reason per failed or unavailable column.
    pub reason: Option<String>,
}

/// A bounded set of categorical values and the number of rows that produced each.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FacetValues {
    /// Kept values in label order.
    pub values: Vec<(CategoryValue, usize)>,
    /// Number of rows whose distinct category was beyond the storage limit.
    pub overflow: usize,
}

/// Accumulates at most 256 category values without sampling claims.
#[derive(Clone, Debug, Default)]
pub struct FacetAccumulator {
    counts: BTreeMap<CategoryValue, usize>,
    overflow: usize,
    pinned: BTreeSet<CategoryValue>,
}

impl FacetAccumulator {
    /// Creates an accumulator that keeps selected values visible even at zero count.
    pub fn with_pinned(pinned: BTreeSet<CategoryValue>) -> Self {
        let counts = pinned.iter().cloned().map(|value| (value, 0)).collect();
        Self {
            counts,
            overflow: 0,
            pinned,
        }
    }

    /// Resumes bounded aggregation from a prior scanned prefix.
    pub fn from_prefix(prefix: FacetValues, pinned: BTreeSet<CategoryValue>) -> Self {
        let mut accumulator = Self::with_pinned(pinned);
        for (value, count) in prefix.values {
            if let Some(existing) = accumulator.counts.get_mut(&value) {
                *existing = count;
            } else if accumulator.counts.len() < 256 {
                accumulator.counts.insert(value, count);
            } else {
                accumulator.overflow += count;
            }
        }
        accumulator.overflow += prefix.overflow;
        accumulator
    }

    /// Adds one successfully acquired displayed value.
    pub fn add(&mut self, value: CategoryValue) {
        if let Some(count) = self.counts.get_mut(&value) {
            *count += 1;
        } else if self.counts.len() < 256 || self.pinned.contains(&value) {
            self.counts.insert(value, 1);
        } else {
            self.overflow += 1;
        }
    }

    /// Returns the values observed, with explicit overflow information.
    pub fn finish(self) -> FacetValues {
        FacetValues {
            values: self.counts.into_iter().collect(),
            overflow: self.overflow,
        }
    }
}

/// Captured listing and rule revisions used to reject stale scan batches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanIdentity {
    /// Revision of rows and listing-backed values.
    pub listing_revision: u64,
    /// Revision of the active rule set.
    pub filter_revision: u64,
}

/// A compact outcome batch keyed by stable row locations.
#[derive(Clone, Debug, Default)]
pub struct OutcomeBatch {
    /// Completed row decisions.
    pub rows: Vec<(Location, RowOutcome)>,
    /// Facets collected independently for each active column.
    pub facets: HashMap<ColumnId, FacetAccumulator>,
    /// Number of rows visited by this batch.
    pub completed: usize,
}

/// Evaluates one row's active rules using only outcomes from that row.
pub fn evaluate_row(
    rules: &[ColumnRule],
    outcomes: &HashMap<ColumnId, ValueOutcome>,
) -> RowOutcome {
    let mut pending = false;
    let mut failed = false;
    let mut unavailable = false;
    for rule in rules {
        match outcomes.get(&rule.column).unwrap_or(&ValueOutcome::Pending) {
            ValueOutcome::Ready { .. } | ValueOutcome::Blank => {
                if !rule.matches(outcomes.get(&rule.column).unwrap_or(&ValueOutcome::Pending)) {
                    return RowOutcome::NoMatch;
                }
            }
            ValueOutcome::Pending => pending = true,
            ValueOutcome::Failed(_) => failed = true,
            ValueOutcome::Unavailable(_) => unavailable = true,
        }
    }
    if unavailable {
        RowOutcome::Unavailable
    } else if failed {
        RowOutcome::Failed
    } else if pending {
        RowOutcome::Pending
    } else {
        RowOutcome::Match
    }
}

/// Per-row compact projection and progress owned by one `Folder` model.
#[derive(Clone, Debug, Default)]
pub struct ColumnFilterState {
    rules: HashMap<ColumnId, ColumnRule>,
    outcomes: HashMap<Location, RowOutcome>,
    facets: HashMap<ColumnId, FacetValues>,
    facet_diagnostics: HashMap<ColumnId, FacetDiagnostics>,
    progress: FilterProgress,
    revision: u64,
}

impl ColumnFilterState {
    /// Current rules in stable column-ID order.
    pub fn rules(&self) -> Vec<ColumnRule> {
        let mut rules: Vec<_> = self.rules.values().cloned().collect();
        rules.sort_by(|a, b| a.column.as_str().cmp(b.column.as_str()));
        rules
    }

    /// Current rule revision, advanced whenever a rule changes.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Current scan progress.
    pub fn progress(&self) -> &FilterProgress {
        &self.progress
    }

    /// Replaces scan counters after a worker reports its current batch totals.
    pub fn set_progress(&mut self, progress: FilterProgress) {
        self.progress = progress;
    }

    /// Current row result, if the scanner has reported one.
    pub fn outcome(&self, location: &Location) -> Option<&RowOutcome> {
        self.outcomes.get(location)
    }

    /// Number of stable row identities with a completed scan decision.
    pub fn decision_count(&self) -> usize {
        self.outcomes.len()
    }

    /// Facet choices collected from the unfiltered listing for one column.
    pub fn facets(&self, column: &ColumnId) -> Option<&FacetValues> {
        self.facets.get(column)
    }

    /// Clones the bounded facet aggregates retained for a scanned prefix.
    pub fn all_facets(&self) -> HashMap<ColumnId, FacetValues> {
        self.facets.clone()
    }

    /// Per-column acquisition diagnostics for category collection.
    pub fn facet_diagnostics(&self, column: &ColumnId) -> Option<&FacetDiagnostics> {
        self.facet_diagnostics.get(column)
    }

    /// Clones per-column acquisition diagnostics retained for a scanned prefix.
    pub fn all_facet_diagnostics(&self) -> HashMap<ColumnId, FacetDiagnostics> {
        self.facet_diagnostics.clone()
    }

    /// Replaces category values after a complete bounded facet scan.
    pub fn set_facets(&mut self, facets: HashMap<ColumnId, FacetValues>) {
        self.facets = facets;
    }

    /// Replaces bounded category-read diagnostics for the current scan.
    pub fn set_facet_diagnostics(&mut self, diagnostics: HashMap<ColumnId, FacetDiagnostics>) {
        self.facet_diagnostics = diagnostics;
    }

    /// Whether filtering is active.
    pub fn is_active(&self) -> bool {
        !self.rules.is_empty()
    }

    /// Replaces all rules and drops decisions tied to the previous revision.
    pub fn set_rules(&mut self, rules: impl IntoIterator<Item = ColumnRule>) -> bool {
        let next: HashMap<_, _> = rules
            .into_iter()
            .filter(|rule| !matches!(&rule.mode, MatchMode::Contains(needle) if needle.is_empty()))
            .map(|rule| (rule.column.clone(), rule))
            .collect();
        if next != self.rules {
            self.rules = next;
            self.revision = self.revision.wrapping_add(1);
            self.outcomes.clear();
            self.facets.clear();
            self.facet_diagnostics.clear();
            self.progress = FilterProgress::default();
            return true;
        }
        false
    }

    /// Removes one rule and invalidates row decisions.
    pub fn clear_column(&mut self, column: &ColumnId) -> bool {
        if self.rules.remove(column).is_some() {
            self.revision = self.revision.wrapping_add(1);
            self.outcomes.clear();
            self.facets.clear();
            self.facet_diagnostics.clear();
            self.progress = FilterProgress::default();
            return true;
        }
        false
    }

    /// Clears all rules and their outcomes.
    pub fn clear(&mut self) {
        self.set_rules(std::iter::empty());
    }

    /// Starts a new scan for the current rules and captured row count.
    pub fn begin(&mut self, total: usize) {
        self.outcomes.clear();
        self.facets.clear();
        self.facet_diagnostics.clear();
        self.progress = FilterProgress {
            total,
            ..FilterProgress::default()
        };
    }

    /// Updates the scan length while retaining prior matches as rows stream in.
    pub fn extend(&mut self, total: usize) {
        self.progress.total = total;
    }

    /// Installs one bounded batch of row decisions and scan progress.
    pub fn apply_batch(&mut self, batch: impl IntoIterator<Item = (Location, RowOutcome)>) {
        for (location, outcome) in batch {
            match outcome {
                RowOutcome::Match => {
                    self.progress.matched += 1;
                    self.progress.completed += 1;
                }
                RowOutcome::NoMatch => self.progress.completed += 1,
                RowOutcome::Pending => self.progress.pending += 1,
                RowOutcome::Failed => {
                    self.progress.failed += 1;
                    self.progress.completed += 1;
                }
                RowOutcome::Unavailable => {
                    self.progress.unavailable += 1;
                    self.progress.completed += 1;
                }
            }
            self.outcomes.insert(location, outcome);
        }
    }

    /// Whether a row is confirmed to pass every active rule.
    pub fn accepts(&self, location: &Location) -> bool {
        !self.is_active() || matches!(self.outcomes.get(location), Some(RowOutcome::Match))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CategoryValue, ColumnRule, DraftMode, FacetAccumulator, FilterDraft, MatchMode, RowOutcome,
        ValueCompleteness, ValueOutcome, evaluate_row,
    };
    use crate::columns::{ColumnId, ColumnKind};
    use std::collections::{BTreeSet, HashMap};

    fn id(kind: ColumnKind) -> ColumnId {
        kind.into()
    }

    fn ready(value: &str) -> ValueOutcome {
        ValueOutcome::Ready {
            text: value.into(),
            completeness: ValueCompleteness::Complete,
        }
    }

    #[test]
    fn substring_is_case_insensitive_and_rules_are_anded() {
        let name = id(ColumnKind::Name);
        let extension = id(ColumnKind::Ext);
        let rules = [
            ColumnRule {
                column: name.clone(),
                mode: MatchMode::Contains("readme".into()),
            },
            ColumnRule {
                column: extension.clone(),
                mode: MatchMode::Contains("md".into()),
            },
        ];
        let mut outcomes = HashMap::from([
            (name.clone(), ready("README.md")),
            (extension.clone(), ready("MD")),
        ]);
        assert_eq!(evaluate_row(&rules, &outcomes), RowOutcome::Match);
        outcomes.insert(extension, ready("txt"));
        assert_eq!(evaluate_row(&rules, &outcomes), RowOutcome::NoMatch);
    }

    #[test]
    fn categories_or_values_and_can_select_blank() {
        let column = id(ColumnKind::GitStatus);
        let rule = ColumnRule {
            column: column.clone(),
            mode: MatchMode::AnyOf(BTreeSet::from([
                CategoryValue::Text("Modified".into()),
                CategoryValue::Blank,
            ])),
        };
        assert!(rule.matches(&ready("Modified")));
        assert!(rule.matches(&ValueOutcome::Blank));
        assert!(!rule.matches(&ready("Added")));
    }

    #[test]
    fn pending_failed_and_unavailable_are_not_blank() {
        let column = id(ColumnKind::Owner);
        let rule = ColumnRule {
            column: column.clone(),
            mode: MatchMode::AnyOf(BTreeSet::from([CategoryValue::Blank])),
        };
        for outcome in [
            ValueOutcome::Pending,
            ValueOutcome::Failed("read failed".into()),
            ValueOutcome::Unavailable("remote".into()),
        ] {
            assert!(!rule.matches(&outcome));
        }
        let rules = [rule];
        assert_eq!(
            evaluate_row(
                &rules,
                &HashMap::from([(column.clone(), ValueOutcome::Failed("permission".into()))])
            ),
            RowOutcome::Failed
        );
        assert_eq!(
            evaluate_row(
                &rules,
                &HashMap::from([(column, ValueOutcome::Unavailable("unsupported".into()))])
            ),
            RowOutcome::Unavailable
        );
    }

    #[test]
    fn partial_values_keep_completeness_and_match_the_displayed_text() {
        let column = id(ColumnKind::Size);
        let rule = ColumnRule {
            column,
            mode: MatchMode::Contains("≥ 1.2".into()),
        };
        assert!(rule.matches(&ValueOutcome::Ready {
            text: "≥ 1.2 G".into(),
            completeness: ValueCompleteness::LowerBound,
        }));
    }

    #[test]
    fn facets_are_bounded_and_keep_blank_distinct() {
        let mut facets = FacetAccumulator::default();
        facets.add(CategoryValue::Blank);
        for index in 0..257 {
            facets.add(CategoryValue::Text(format!("value-{index:03}")));
        }
        let facets = facets.finish();
        assert_eq!(facets.values.len(), 256);
        assert_eq!(facets.overflow, 2);
        assert!(facets.values.contains(&(CategoryValue::Blank, 1)));
    }

    #[test]
    fn draft_modes_preserve_choices_while_switching_between_text_and_values() {
        let column = id(ColumnKind::Owner);
        let selected = BTreeSet::from([CategoryValue::Text("jan".into())]);
        let draft = FilterDraft {
            mode: DraftMode::Values,
            query: "ja".into(),
            selected: selected.clone(),
        };
        assert_eq!(
            draft.rule(column.clone()).map(|rule| rule.mode),
            Some(MatchMode::AnyOf(selected.clone()))
        );
        let text = FilterDraft {
            mode: DraftMode::Text,
            ..draft.clone()
        };
        assert_eq!(
            text.rule(column.clone()).map(|rule| rule.mode),
            Some(MatchMode::Contains("ja".into()))
        );
        let values_again = FilterDraft {
            mode: DraftMode::Values,
            ..text
        };
        assert_eq!(values_again.selected, selected);
    }

    #[test]
    fn empty_text_draft_clears_instead_of_matching_blank() {
        let draft = FilterDraft {
            mode: DraftMode::Text,
            ..FilterDraft::default()
        };
        assert_eq!(draft.rule(id(ColumnKind::Name)), None);
    }
}
