//! Independent, cancellable acquisition for values used by column filters.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use crate::column_filter::{
    CategoryValue, ColumnRule, FacetAccumulator, FacetDiagnostics, FacetValues, FilterProgress,
    MatchMode, RowOutcome, ValueOutcome, evaluate_row,
};
use crate::columns::{Catalog, ColumnId, ColumnKind, FilterBehavior};
use crate::data_providers::git;
use crate::model::Row;

/// Whether an identical demand can reuse a live worker or completed result.
fn scan_demand_reusable(same_key: bool, completed: bool, worker: Option<&AtomicBool>) -> bool {
    same_key && (completed || worker.is_some_and(|cancel| !cancel.load(Ordering::Relaxed)))
}

/// Captured context for one row, kept independent of table virtualization.
#[derive(Clone)]
pub struct CapturedRow {
    /// Stable row identity and listing values.
    pub row: Row,
    /// Exact Size-cell text for a measured directory, if available.
    pub directory_size: Option<String>,
}

/// One bounded scan result, containing no raw metadata values.
pub struct ScanBatch {
    /// Compact decisions from at most 64 rows.
    pub rows: Vec<(crate::location::Location, RowOutcome)>,
    /// Progress through the captured, unfiltered listing.
    pub progress: FilterProgress,
    /// Present only on the final batch.
    pub facets: Option<HashMap<ColumnId, FacetValues>>,
    /// Acquisition health, independent of row-match decisions.
    pub diagnostics: Option<HashMap<ColumnId, FacetDiagnostics>>,
    /// Whether the captured listing has been exhausted.
    pub finished: bool,
}

/// Bounded aggregate state retained when a streamed listing adds rows.
pub struct ScanPrefix {
    /// Previously completed row decisions and diagnostics.
    pub progress: FilterProgress,
    /// Previously collected bounded category values.
    pub facets: HashMap<ColumnId, FacetValues>,
    /// Previously collected per-column read failures.
    pub diagnostics: HashMap<ColumnId, FacetDiagnostics>,
    /// Current full listing length for progress reporting.
    pub total: usize,
}

/// One panel's sequential scan state; each call reads at most 64 listing rows.
pub struct ScanSession {
    rows: Vec<CapturedRow>,
    rules: Vec<ColumnRule>,
    facet_columns: Vec<ColumnId>,
    catalog: Arc<Catalog>,
    git_result: Option<Result<Option<Arc<git::Snapshot>>, String>>,
    cancel: Arc<AtomicBool>,
    cursor: usize,
    finalized: bool,
    progress: FilterProgress,
    accumulators: HashMap<ColumnId, FacetAccumulator>,
    diagnostics: HashMap<ColumnId, FacetDiagnostics>,
}

impl ScanSession {
    /// Captures all providers and rules needed for one cancellable panel scan.
    #[cfg(test)]
    pub fn new(
        rows: Vec<CapturedRow>,
        rules: Vec<ColumnRule>,
        facet_columns: Vec<ColumnId>,
        catalog: Arc<Catalog>,
        git_result: Option<Result<Option<Arc<git::Snapshot>>, String>>,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        let total = rows.len();
        Self::new_with_prefix(
            rows,
            rules,
            facet_columns,
            catalog,
            git_result,
            cancel,
            ScanPrefix {
                progress: FilterProgress {
                    total,
                    ..FilterProgress::default()
                },
                facets: HashMap::new(),
                diagnostics: HashMap::new(),
                total,
            },
        )
    }

    /// Continues scanning an appended suffix while retaining bounded prefix results.
    pub fn new_with_prefix(
        rows: Vec<CapturedRow>,
        rules: Vec<ColumnRule>,
        facet_columns: Vec<ColumnId>,
        catalog: Arc<Catalog>,
        git_result: Option<Result<Option<Arc<git::Snapshot>>, String>>,
        cancel: Arc<AtomicBool>,
        prefix: ScanPrefix,
    ) -> Self {
        let ScanPrefix {
            mut progress,
            facets: prefixes,
            diagnostics,
            total,
        } = prefix;
        let mut accumulators = HashMap::new();
        for rule in &rules {
            if let MatchMode::AnyOf(values) = &rule.mode {
                accumulators.insert(
                    rule.column.clone(),
                    FacetAccumulator::from_prefix(
                        prefixes.get(&rule.column).cloned().unwrap_or_default(),
                        values.clone(),
                    ),
                );
            }
        }
        for column in &facet_columns {
            accumulators.entry(column.clone()).or_insert_with(|| {
                FacetAccumulator::from_prefix(
                    prefixes.get(column).cloned().unwrap_or_default(),
                    BTreeSet::new(),
                )
            });
        }
        progress.total = total;
        Self {
            rows,
            rules,
            facet_columns,
            catalog,
            git_result,
            cancel,
            cursor: 0,
            finalized: false,
            progress,
            accumulators,
            diagnostics,
        }
    }

    /// Acquires one bounded row batch and checks cancellation between reads.
    pub fn next_batch(&mut self) -> Option<ScanBatch> {
        if self.cancel.load(Ordering::Relaxed) || self.finalized {
            return None;
        }
        if self.cursor == self.rows.len() {
            self.finalized = true;
            let facets = std::mem::take(&mut self.accumulators)
                .into_iter()
                .map(|(column, values)| (column, values.finish()))
                .collect();
            return Some(ScanBatch {
                rows: Vec::new(),
                progress: self.progress.clone(),
                facets: Some(facets),
                diagnostics: Some(std::mem::take(&mut self.diagnostics)),
                finished: true,
            });
        }
        let start = self.cursor;
        let end = (start + 64).min(self.rows.len());
        let started = Instant::now();
        let mut decisions = Vec::with_capacity(end - start);
        while self.cursor < end {
            if self.cursor > start && started.elapsed() >= Duration::from_millis(150) {
                break;
            }
            if self.cancel.load(Ordering::Relaxed) {
                return None;
            }
            let captured = &self.rows[self.cursor];
            self.cursor += 1;
            let row = &captured.row;
            if row.is_parent {
                decisions.push((row.location.clone(), RowOutcome::Match));
                self.progress.matched += 1;
                self.progress.completed += 1;
                continue;
            }
            let mut values = HashMap::new();
            for rule in &self.rules {
                if self.cancel.load(Ordering::Relaxed) {
                    return None;
                }
                let value = self.read_value(rule, captured);
                if let Some(facets) = self.accumulators.get_mut(&rule.column) {
                    record_diagnostic(
                        self.diagnostics.entry(rule.column.clone()).or_default(),
                        &value,
                    );
                    match &value {
                        ValueOutcome::Ready { text, .. } => {
                            facets.add(CategoryValue::Text(text.clone()))
                        }
                        ValueOutcome::Blank => facets.add(CategoryValue::Blank),
                        ValueOutcome::Pending
                        | ValueOutcome::Failed(_)
                        | ValueOutcome::Unavailable(_) => {}
                    }
                }
                values.insert(rule.column.clone(), value);
            }
            for column in &self.facet_columns {
                if values.contains_key(column) || self.cancel.load(Ordering::Relaxed) {
                    continue;
                }
                let probe = ColumnRule {
                    column: column.clone(),
                    mode: MatchMode::Contains(String::new()),
                };
                let value = self.read_value(&probe, captured);
                if let Some(facets) = self.accumulators.get_mut(column) {
                    record_diagnostic(self.diagnostics.entry(column.clone()).or_default(), &value);
                    match &value {
                        ValueOutcome::Ready { text, .. } => {
                            facets.add(CategoryValue::Text(text.clone()))
                        }
                        ValueOutcome::Blank => facets.add(CategoryValue::Blank),
                        ValueOutcome::Pending
                        | ValueOutcome::Failed(_)
                        | ValueOutcome::Unavailable(_) => {}
                    }
                }
            }
            let outcome = evaluate_row(&self.rules, &values);
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
            decisions.push((row.location.clone(), outcome));
        }
        let finished = self.cursor == self.rows.len();
        self.finalized = finished;
        let facets = finished.then(|| {
            std::mem::take(&mut self.accumulators)
                .into_iter()
                .map(|(column, values)| (column, values.finish()))
                .collect()
        });
        let diagnostics = finished.then(|| std::mem::take(&mut self.diagnostics));
        Some(ScanBatch {
            rows: decisions,
            progress: self.progress.clone(),
            facets,
            diagnostics,
            finished,
        })
    }

    fn read_value(&self, rule: &ColumnRule, captured: &CapturedRow) -> ValueOutcome {
        let row = &captured.row;
        let Some(descriptor) = self.catalog.descriptor(&rule.column) else {
            return ValueOutcome::Unavailable("Provider unavailable".into());
        };
        if let FilterBehavior::Disabled(reason) = &descriptor.filter_behavior {
            return ValueOutcome::Unavailable(reason.clone());
        }
        match rule.column.builtin() {
            Some(ColumnKind::GitStatus) => match &self.git_result {
                None => ValueOutcome::Pending,
                Some(Err(error)) => ValueOutcome::Failed(error.clone()),
                Some(Ok(None)) => {
                    ValueOutcome::Unavailable("No Git repository for this location".into())
                }
                Some(Ok(Some(snapshot))) => match snapshot.mark(&row.path, row.is_dir) {
                    Some(mark) => ready(mark.symbol().to_owned()),
                    None => ValueOutcome::Blank,
                },
            },
            Some(kind) => match crate::columns::display::listing_text(
                kind,
                &row.entry,
                captured.directory_size.as_deref(),
            ) {
                Some(text) => {
                    if text.is_empty() {
                        ValueOutcome::Blank
                    } else {
                        ready(text)
                    }
                }
                None => crate::panel::metadata::reader::read_filter_column(
                    &rule.column,
                    row,
                    &self.cancel,
                    &self.catalog,
                ),
            },
            None => crate::panel::metadata::reader::read_filter_column(
                &rule.column,
                row,
                &self.cancel,
                &self.catalog,
            ),
        }
    }
}

fn record_diagnostic(diagnostic: &mut FacetDiagnostics, value: &ValueOutcome) {
    match value {
        ValueOutcome::Pending => diagnostic.pending += 1,
        ValueOutcome::Failed(reason) => {
            diagnostic.failed += 1;
            diagnostic.reason.get_or_insert_with(|| reason.clone());
        }
        ValueOutcome::Unavailable(reason) => {
            diagnostic.unavailable += 1;
            diagnostic.reason.get_or_insert_with(|| reason.clone());
        }
        ValueOutcome::Ready { .. } | ValueOutcome::Blank => {}
    }
}

fn ready(text: String) -> ValueOutcome {
    ValueOutcome::Ready {
        text,
        completeness: crate::column_filter::ValueCompleteness::Complete,
    }
}

impl super::FolderView {
    /// Opens the captured column's filter editor and starts bounded facet collection when useful.
    pub fn open_column_filter(
        &mut self,
        column: ColumnId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let descriptor = self.catalog.display_descriptor(&column);
        let folder = self.folder.read(cx);
        let wants_categories = matches!(
            descriptor.filter_behavior,
            crate::columns::FilterBehavior::Categories
        );
        let draft = crate::panel::column_filter_view::initial_rule(
            &folder.column_filter().rules(),
            &column,
            wants_categories,
        );
        let source = folder.content().clone();
        if self.filter_facet_column != wants_categories.then_some(column.clone()) {
            self.filter_scan_request_revision = self.filter_scan_request_revision.wrapping_add(1);
        }
        self.filter_facet_column = wants_categories.then_some(column.clone());
        self.request_column_filter_scan(cx);
        let editor = cx.new(|cx| {
            crate::panel::column_filter_view::ColumnFilterEditor::new(
                self.folder.clone(),
                source,
                column,
                descriptor.label,
                draft,
                window,
                cx,
            )
        });
        let weak = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let editor = editor.clone();
            let weak = weak.clone();
            dialog
                .title("Filter column")
                .w(px(460.))
                .child(editor)
                .on_close(move |_, _, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        if this.filter_facet_column.take().is_some() {
                            this.filter_scan_request_revision =
                                this.filter_scan_request_revision.wrapping_add(1);
                            this.request_column_filter_scan(cx);
                        }
                    });
                })
        });
    }

    /// Captures local Git demand and schedules shared bounded work off the UI thread.
    pub fn refresh_git(&mut self, cx: &mut Context<Self>) {
        let folder = self.folder.read(cx);
        let target = self
            .table
            .read(cx)
            .delegate()
            .columns()
            .iter()
            .any(|column| column.as_str() == self.git_service.definition().columns[0].column_id)
            .then(|| folder.location().cloned())
            .flatten()
            .and_then(|location| {
                let scope = location.as_local_path()?;
                Some((
                    location.clone(),
                    folder.generation(),
                    self.git_service.key_for_scope(scope),
                ))
            });
        let request = self.git_session.sync(target);
        if request.is_none() {
            // A disabled/nonlocal demand must also clear prepared marks.
            let is_local = folder
                .location()
                .and_then(|location| location.as_local_path())
                .is_some();
            let is_visible = self
                .table
                .read(cx)
                .delegate()
                .columns()
                .iter()
                .any(|column| {
                    column.as_str() == self.git_service.definition().columns[0].column_id
                });
            if !is_local || !is_visible {
                let changed = if !is_local {
                    if matches!(&self.git_filter_result, Some(Err(message)) if message == "Git status requires a local folder")
                    {
                        false
                    } else {
                        self.git_filter_result =
                            Some(Err("Git status requires a local folder".to_owned()));
                        true
                    }
                } else {
                    let changed = self.git_filter_result.is_some();
                    self.git_filter_result = None;
                    changed
                };
                if changed {
                    self.git_filter_revision = self.git_filter_revision.wrapping_add(1);
                }
                self.table.update(cx, |table, cx| {
                    table.delegate_mut().set_git_snapshot(None);
                    cx.notify();
                });
                self.request_column_filter_scan(cx);
            }
            return;
        }
        let request = request.expect("a changed local demand returned a request");
        self.table.update(cx, |table, cx| {
            table.delegate_mut().set_git_snapshot(None);
            cx.notify();
        });
        self.git_filter_result = None;
        self.git_filter_revision = self.git_filter_revision.wrapping_add(1);
        self.request_column_filter_scan(cx);
        let lease = self.git_service.subscribe(request.key.clone());
        self._git_task = cx.spawn(async move |this, cx| {
            let worker_request = request.clone();
            let result = cx
                .background_spawn(async move {
                    lease.and_then(|lease| lease.wait(&worker_request.cancel))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let folder = this.folder.read(cx);
                if !this.git_session.accepts(&request)
                    || folder.generation() != request.generation
                    || folder.location() != Some(&request.location)
                    || this.git_service.key_for_scope(&request.key.scope) != request.key
                {
                    return;
                }
                let snapshot = result.as_ref().ok().cloned().flatten();
                this.git_filter_result = Some(result.map_err(|error| error.to_string()));
                this.git_filter_revision = this.git_filter_revision.wrapping_add(1);
                this.table.update(cx, |table, cx| {
                    table.delegate_mut().set_git_snapshot(snapshot);
                    cx.notify();
                });
                this.request_column_filter_scan(cx);
            });
        });
    }

    /// Starts or replaces the panel's single independent column-value scan.
    pub fn request_column_filter_scan(&mut self, cx: &mut Context<Self>) {
        let folder = self.folder.read(cx);
        let identity = folder.column_filter_identity();
        let rules = folder.column_filter().rules();
        let facet_column = self.filter_facet_column.clone();
        let wants_git = rules
            .iter()
            .any(|rule| rule.column.builtin() == Some(ColumnKind::GitStatus))
            || facet_column
                .as_ref()
                .is_some_and(|column| column.builtin() == Some(ColumnKind::GitStatus));
        let git_revision = if wants_git {
            self.git_filter_revision
        } else {
            0
        };
        let request_revision = self.filter_scan_request_revision;
        if rules.is_empty() && facet_column.is_none() {
            if let Some(cancel) = &self.filter_scan_cancel {
                cancel.store(true, Ordering::Relaxed);
                self.filter_scan_pending = false;
            }
            self.filter_scan_identity = None;
            self.filter_scan_done = true;
            return;
        }
        let same_key = self.filter_scan_identity == Some(identity)
            && self.filter_scan_facet == facet_column
            && self.filter_scan_git_revision == git_revision
            && self.filter_scan_active_request_revision == request_revision;
        let complete_for_current =
            self.filter_scan_done && self.filter_scan_rows >= folder.all_entries().len();
        if scan_demand_reusable(
            same_key,
            complete_for_current,
            self.filter_scan_cancel.as_deref(),
        ) {
            return;
        }
        if let Some(cancel) = &self.filter_scan_cancel {
            cancel.store(true, Ordering::Relaxed);
            self.filter_scan_pending = true;
            return;
        }
        let resume_prefix = self.filter_scan_identity == Some(identity)
            && self.filter_scan_facet == facet_column
            && self.filter_scan_git_revision == git_revision
            && self.filter_scan_active_request_revision == request_revision
            && self.filter_scan_rows <= folder.all_entries().len();
        let prefix_rows = if resume_prefix {
            folder.column_filter().decision_count()
        } else {
            0
        };
        let prefixes = if resume_prefix {
            folder.column_filter().all_facets()
        } else {
            Default::default()
        };
        let diagnostics = if resume_prefix {
            folder.column_filter().all_facet_diagnostics()
        } else {
            Default::default()
        };
        let progress = if resume_prefix {
            folder.column_filter().progress().clone()
        } else {
            crate::column_filter::FilterProgress::default()
        };
        let captured: Vec<_> = folder
            .all_entries()
            .iter()
            .filter(|row| !resume_prefix || folder.column_filter().outcome(&row.location).is_none())
            .cloned()
            .map(|row| {
                let directory_size = row
                    .is_dir
                    .then(|| folder.dir_size(&row.path).map(|size| size.label()))
                    .flatten();
                crate::panel::column_filter::CapturedRow {
                    row,
                    directory_size,
                }
            })
            .collect();
        let listing_total = folder.all_entries().len();
        let catalog = self.catalog.clone();
        let git_result = self.git_filter_result.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        self.filter_scan_cancel = Some(cancel.clone());
        self.filter_scan_identity = Some(identity);
        self.filter_scan_facet = facet_column.clone();
        self.filter_scan_git_revision = git_revision;
        self.filter_scan_active_request_revision = request_revision;
        self.filter_scan_done = false;
        self.filter_scan_pending = false;
        if !resume_prefix {
            self.filter_scan_rows = 0;
        } else {
            self.filter_scan_rows = prefix_rows;
        }
        self.folder.update(cx, |folder, cx| {
            folder.begin_column_filter_scan(identity, listing_total);
            cx.notify();
        });
        self._filter_scan_task = cx.spawn(async move |this, cx| {
            let mut session = crate::panel::column_filter::ScanSession::new_with_prefix(
                captured,
                rules,
                facet_column.into_iter().collect(),
                catalog,
                git_result,
                cancel.clone(),
                crate::panel::column_filter::ScanPrefix {
                    progress,
                    facets: prefixes,
                    diagnostics,
                    total: listing_total,
                },
            );
            let mut processed_rows = prefix_rows;
            loop {
                let (next_session, batch) = cx
                    .background_spawn(async move {
                        let mut session = session;
                        let batch = session.next_batch();
                        (session, batch)
                    })
                    .await;
                session = next_session;
                let Some(batch) = batch else {
                    break;
                };
                let finished = batch.finished;
                let processed = batch.rows.len();
                processed_rows = processed_rows.saturating_add(processed);
                let processed_total = processed_rows;
                let _ = this.update(cx, |this, cx| {
                    let same_worker = this
                        .filter_scan_cancel
                        .as_ref()
                        .is_some_and(|active| Arc::ptr_eq(active, &cancel));
                    if !same_worker
                        || cancel.load(Ordering::Relaxed)
                        || this.folder.read(cx).column_filter_identity() != identity
                        || (wants_git && this.git_filter_revision != git_revision)
                        || this.filter_scan_request_revision != request_revision
                    {
                        return;
                    }
                    this.filter_scan_rows = processed_total;
                    this.filter_scan_done = finished;
                    this.folder.update(cx, |folder, cx| {
                        folder.apply_column_filter_batch(identity, batch.rows);
                        let mut progress = batch.progress.clone();
                        progress.total = folder.all_entries().len();
                        folder.set_column_filter_progress(progress.clone());
                        if let (Some(facets), Some(diagnostics)) = (batch.facets, batch.diagnostics)
                        {
                            folder.complete_column_filter_scan(
                                identity,
                                facets,
                                diagnostics,
                                progress,
                            );
                            folder.finish_column_filter_scan();
                        }
                        cx.notify();
                    });
                });
                if finished {
                    break;
                }
            }
            let _ = this.update(cx, |this, cx| {
                let same_worker = this
                    .filter_scan_cancel
                    .as_ref()
                    .is_some_and(|active| Arc::ptr_eq(active, &cancel));
                if same_worker {
                    this.filter_scan_cancel = None;
                    if cancel.load(Ordering::Relaxed) || !this.filter_scan_done {
                        this.filter_scan_identity = None;
                    }
                }
                let has_appended_rows =
                    this.folder.read(cx).all_entries().len() > this.filter_scan_rows;
                if same_worker && (this.filter_scan_pending || has_appended_rows) {
                    this.filter_scan_pending = false;
                    this.request_column_filter_scan(cx);
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{CapturedRow, ScanSession, scan_demand_reusable};
    use crate::column_filter::{ColumnRule, MatchMode, RowOutcome};
    use crate::columns::{Catalog, ColumnId, ColumnKind};
    use crate::model::test_support::{fixture, loaded};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn demand_reuses_only_completed_or_live_uncancelled_workers() {
        let live = AtomicBool::new(false);
        let cancelled = AtomicBool::new(true);
        assert!(scan_demand_reusable(true, false, Some(&live)));
        assert!(scan_demand_reusable(true, true, None));
        assert!(!scan_demand_reusable(true, false, Some(&cancelled)));
        assert!(!scan_demand_reusable(false, true, None));
    }

    #[test]
    fn scans_all_captured_rows_and_keeps_parent() {
        let dir = fixture();
        let folder = loaded(&dir);
        let rows: Vec<_> = folder
            .all_entries()
            .iter()
            .cloned()
            .map(|row| CapturedRow {
                row,
                directory_size: None,
            })
            .collect();
        let rules = [ColumnRule {
            column: ColumnId::from(ColumnKind::Name),
            mode: MatchMode::Contains("a.txt".into()),
        }];
        let mut session = ScanSession::new(
            rows.clone(),
            rules.to_vec(),
            Vec::new(),
            Arc::new(Catalog::with_defaults()),
            None,
            Arc::new(AtomicBool::new(false)),
        );
        let mut decisions = Vec::new();
        let mut final_progress = None;
        while let Some(batch) = session.next_batch() {
            assert!(batch.rows.len() <= 64);
            decisions.extend(batch.rows);
            if batch.finished {
                final_progress = Some(batch.progress);
            }
        }
        assert_eq!(final_progress.expect("scan completes").total, rows.len());
        assert!(
            decisions
                .iter()
                .any(|(_, outcome)| *outcome == RowOutcome::Match)
        );
        assert_eq!(decisions[0].1, RowOutcome::Match);
    }

    #[test]
    fn finds_a_match_after_the_render_and_metadata_cache_windows() {
        let dir = fixture();
        let folder = loaded(&dir);
        let template = folder
            .all_entries()
            .iter()
            .find(|row| !row.is_parent)
            .expect("fixture row");
        let rows: Vec<_> = (0..600)
            .map(|index| {
                let mut row = template.clone();
                row.entry.name = if index == 599 {
                    "needle-at-end.bin".into()
                } else {
                    format!("row-{index}.bin")
                };
                row.entry.path = dir.path().join(&row.entry.name);
                row.location = crate::location::Location::local(row.entry.path.clone())
                    .expect("absolute test path");
                CapturedRow {
                    row,
                    directory_size: None,
                }
            })
            .collect();
        let mut session = ScanSession::new(
            rows,
            vec![ColumnRule {
                column: ColumnId::from(ColumnKind::Name),
                mode: MatchMode::Contains("needle-at-end".into()),
            }],
            Vec::new(),
            Arc::new(Catalog::with_defaults()),
            None,
            Arc::new(AtomicBool::new(false)),
        );
        let mut last_match = None;
        while let Some(batch) = session.next_batch() {
            assert!(batch.rows.len() <= 64);
            last_match = batch
                .rows
                .into_iter()
                .rfind(|(_, outcome)| *outcome == RowOutcome::Match)
                .or(last_match);
            if batch.finished {
                break;
            }
        }
        assert_eq!(
            last_match
                .expect("match past row 512")
                .0
                .as_local_path()
                .expect("local")
                .file_name()
                .unwrap(),
            "needle-at-end.bin"
        );
    }

    #[test]
    fn cancelled_scan_publishes_no_more_batches() {
        let dir = fixture();
        let folder = loaded(&dir);
        let rows = folder
            .all_entries()
            .iter()
            .cloned()
            .map(|row| CapturedRow {
                row,
                directory_size: None,
            })
            .collect();
        let cancel = Arc::new(AtomicBool::new(true));
        let mut session = ScanSession::new(
            rows,
            Vec::new(),
            Vec::new(),
            Arc::new(Catalog::with_defaults()),
            None,
            cancel,
        );
        assert!(session.next_batch().is_none());
    }
}
