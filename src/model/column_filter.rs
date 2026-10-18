//! Folder-owned column-filter rules, scan identity, and row projections.

use super::Folder;
use crate::column_filter::{
    ColumnFilterState, ColumnRule, FilterProgress, RowOutcome, ScanIdentity,
};
use crate::columns::{ColumnId, ColumnKind};
use crate::location::Location;

impl Folder {
    /// Captures the revisions that make a column-filter scan current.
    pub fn column_filter_identity(&self) -> ScanIdentity {
        ScanIdentity {
            listing_revision: self.column_scan_revision,
            filter_revision: self.column_filter.revision(),
        }
    }

    /// All captured rows before name, hidden-file, and column filters.
    pub fn all_entries(&self) -> &[super::Row] {
        &self.all
    }

    /// Rules and progress for the active column filters.
    pub fn column_filter(&self) -> &ColumnFilterState {
        &self.column_filter
    }

    /// Replaces all column rules and drops decisions from older scans.
    pub fn set_column_filters(&mut self, rules: Vec<ColumnRule>) {
        let selected = self.current().map(|row| row.location.clone());
        self.column_filter_cursor = self.column_filter_cursor.clone().or(selected.clone());
        if !self.column_filter.set_rules(rules) {
            return;
        }
        self.column_filter.begin(self.all.len());
        self.apply_filter();
        if !self.select_location(selected.as_ref()) {
            self.cursor = self
                .entries
                .iter()
                .position(|row| !row.is_parent)
                .unwrap_or(0);
        }
    }

    /// Removes one column's filter and retains other rules.
    pub fn clear_column_filter(&mut self, column: &ColumnId) {
        let selected = self.current().map(|row| row.location.clone());
        self.column_filter_cursor = self.column_filter_cursor.clone().or(selected.clone());
        if !self.column_filter.clear_column(column) {
            return;
        }
        if !self.column_filter.is_active() {
            self.column_filter_cursor = None;
        }
        self.column_filter.begin(self.all.len());
        self.apply_filter();
        self.select_location(selected.as_ref());
    }

    /// Removes a rule when its column is hidden.
    pub fn clear_hidden_column_filter(&mut self, visible: &[ColumnId]) {
        let hidden: Vec<_> = self
            .column_filter
            .rules()
            .into_iter()
            .filter(|rule| !visible.contains(&rule.column))
            .map(|rule| rule.column)
            .collect();
        for column in hidden {
            self.clear_column_filter(&column);
        }
    }

    /// Clears all column filters while keeping the name filter.
    pub fn clear_column_filters(&mut self) {
        let selected = self.current().map(|row| row.location.clone());
        self.column_filter.clear();
        self.column_filter_cursor = None;
        self.apply_filter();
        self.select_location(selected.as_ref());
    }

    /// Starts a scan for the current rules and clears earlier decisions.
    pub fn begin_column_filter_scan(&mut self, identity: ScanIdentity, total: usize) -> bool {
        if self.column_filter_identity() != identity {
            return false;
        }
        self.column_filter.extend(total);
        true
    }

    /// Applies one batch of compact decisions if the scan identity still matches.
    pub fn apply_column_filter_batch(
        &mut self,
        identity: ScanIdentity,
        rows: Vec<(Location, RowOutcome)>,
    ) -> bool {
        if self.column_filter_identity() != identity {
            return false;
        }
        let selected = self.current().map(|row| row.location.clone());
        self.column_filter.apply_batch(rows);
        self.apply_filter();
        let cursor_anchor = self.column_filter_cursor.clone();
        if self.select_location(cursor_anchor.as_ref()) {
            self.column_filter_cursor = None;
        } else {
            self.select_location(selected.as_ref());
        }
        true
    }

    /// Finishes a scan and falls back to the first matching row if the former cursor was removed.
    pub fn finish_column_filter_scan(&mut self) {
        if self.column_filter_cursor.take().is_some() {
            self.cursor = self
                .entries
                .iter()
                .position(|row| !row.is_parent)
                .unwrap_or(0);
        }
    }

    /// Updates scan totals after a batch has finished.
    pub fn set_column_filter_progress(&mut self, progress: FilterProgress) {
        self.column_filter.set_progress(progress);
    }

    /// Installs one completed scan's facets and counters for its captured identity.
    pub fn complete_column_filter_scan(
        &mut self,
        identity: ScanIdentity,
        facets: std::collections::HashMap<ColumnId, crate::column_filter::FacetValues>,
        diagnostics: std::collections::HashMap<ColumnId, crate::column_filter::FacetDiagnostics>,
        progress: FilterProgress,
    ) -> bool {
        if self.column_filter_identity() != identity {
            return false;
        }
        self.column_filter.set_facets(facets);
        self.column_filter.set_facet_diagnostics(diagnostics);
        self.column_filter.set_progress(progress);
        true
    }

    /// Invalidates only rules whose displayed value depends on measured sizes.
    pub fn invalidate_size_filter_rows(&mut self) {
        if self
            .column_filter
            .rules()
            .iter()
            .any(|rule| rule.column.builtin() == Some(ColumnKind::Size))
        {
            let selected = self.current().map(|row| row.location.clone());
            self.column_filter_cursor = self.column_filter_cursor.clone().or(selected.clone());
            self.listing_revision = self.listing_revision.wrapping_add(1);
            self.column_scan_revision = self.column_scan_revision.wrapping_add(1);
            self.column_filter.begin(self.all.len());
            self.apply_filter();
            let anchor = self.column_filter_cursor.clone();
            if !self.select_location(anchor.as_ref()) {
                self.select_location(selected.as_ref());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column_filter::{MatchMode, RowOutcome};
    use crate::columns::ColumnKind;
    use crate::entry::{Sort, SortKey};
    use crate::model::test_support::{fixture, loaded, reload};

    fn name_rule(value: &str) -> ColumnRule {
        ColumnRule {
            column: ColumnKind::Name.into(),
            mode: MatchMode::Contains(value.into()),
        }
    }

    #[test]
    fn identical_rules_are_a_noop_and_refresh_keeps_the_rule() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        let rule = name_rule("a");
        folder.set_column_filters(vec![rule.clone()]);
        let revision = folder.column_filter_identity().filter_revision;
        folder.set_column_filters(vec![rule.clone()]);
        assert_eq!(folder.column_filter_identity().filter_revision, revision);
        reload(&mut folder);
        assert_eq!(folder.column_filter().rules(), vec![rule]);
    }

    #[test]
    fn navigation_and_new_search_clear_column_rules() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder
            .navigate(dir.path().join("sub"))
            .expect("navigate to subfolder");
        folder.set_column_filters(vec![name_rule("a")]);
        folder.go_up().expect("navigate to parent");
        assert!(folder.column_filter().rules().is_empty());
        folder.set_column_filters(vec![name_rule("b")]);
        folder.show_results(dir.path().to_path_buf(), "new search".into());
        assert!(folder.column_filter().rules().is_empty());
    }

    #[test]
    fn appending_search_rows_retains_confirmed_decisions() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.show_results(dir.path().to_path_buf(), "search".into());
        folder.set_column_filters(vec![name_rule("keep")]);
        let make_hit = |name: &str| crate::search::Hit {
            entry: crate::entry::Entry {
                name: name.into(),
                path: dir.path().join(name),
                is_dir: false,
                is_parent: false,
                size: Some(1),
                modified: None,
                attrs: Default::default(),
                link: Default::default(),
            },
            line: None,
        };
        let scan_identity = folder.column_filter_identity();
        let listing_revision = folder.column_filter_identity().listing_revision;
        folder.add_results(vec![make_hit("keep-me.txt")]);
        assert_eq!(folder.column_filter_identity(), scan_identity);
        assert_eq!(
            folder.column_filter_identity().listing_revision,
            listing_revision
        );
        let first = folder
            .all_entries()
            .last()
            .expect("first hit")
            .location
            .clone();
        let identity = folder.column_filter_identity();
        assert!(
            folder.apply_column_filter_batch(identity, vec![(first.clone(), RowOutcome::Match)])
        );
        folder.add_results(vec![make_hit("0-keep-later.txt")]);
        folder.set_sort(Sort {
            key: SortKey::Name,
            descending: false,
        });
        assert!(folder.column_filter().accepts(&first));
        assert!(folder.entries().iter().any(|row| row.location == first));
        let undecided: Vec<_> = folder
            .all_entries()
            .iter()
            .filter(|row| folder.column_filter().outcome(&row.location).is_none())
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(undecided, ["..", "0-keep-later.txt"]);
    }

    #[test]
    fn size_updates_invalidate_only_size_dependent_rules() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.set_column_filters(vec![name_rule("a")]);
        let revision = folder.column_filter_identity().listing_revision;
        folder.invalidate_size_filter_rows();
        assert_eq!(folder.column_filter_identity().listing_revision, revision);
        folder.set_column_filters(vec![ColumnRule {
            column: ColumnKind::Size.into(),
            mode: MatchMode::Contains("1".into()),
        }]);
        let revision = folder.column_filter_identity().listing_revision;
        folder.invalidate_size_filter_rows();
        assert_eq!(
            folder.column_filter_identity().listing_revision,
            revision + 1
        );
    }

    #[test]
    fn swap_moves_column_rules_with_panel_contents() {
        let left = fixture();
        let right = fixture();
        let mut left = loaded(&left);
        let mut right = loaded(&right);
        let left_rule = name_rule("left");
        let right_rule = name_rule("right");
        left.set_column_filters(vec![left_rule.clone()]);
        right.set_column_filters(vec![right_rule.clone()]);
        left.swap_with(&mut right);
        assert_eq!(left.column_filter().rules(), vec![right_rule]);
        assert_eq!(right.column_filter().rules(), vec![left_rule]);
        assert_ne!(left.column_filter_identity().listing_revision, 0);
    }
}
