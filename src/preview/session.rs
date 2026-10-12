//! Preview lifecycle and navigation decisions, independent of GUI scheduling.

use std::path::PathBuf;

use crate::location::Location;

use super::{Content, Preview, Sheet};

/// Current preview presentation data.
#[derive(Debug, Default)]
pub enum State {
    /// No readable target is selected.
    #[default]
    Empty,
    /// The selected target is being read.
    Loading,
    /// The selected target has been read.
    Ready(Preview),
    /// Reading the selected target failed.
    Failed(String),
}

/// Local bytes and their original typed identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewTarget {
    /// Materialized path passed to readers, if available.
    pub path: Option<PathBuf>,
    /// Original local, remote or archive item.
    pub identity: Option<Location>,
}

/// A versioned request for background preview work.
#[derive(Clone, Debug)]
pub struct Request {
    /// Path passed to the reader.
    pub path: PathBuf,
    /// PDF page (one based) or sheet index (zero based); zero for a full load.
    pub index: usize,
    generation: u64,
}

/// Effect of changing the selected target or search-hit line.
pub enum TargetChange {
    /// Selection and line are unchanged.
    Unchanged,
    /// Only the requested text line changed.
    Reveal,
    /// Reset GUI resources and optionally schedule a load.
    Reset(Option<Request>),
}

/// Owns target identity, navigation and acceptance of background results.
pub struct PreviewSession {
    target: Option<PreviewTarget>,
    line: Option<u64>,
    state: State,
    generation: u64,
    pdf_generation: u64,
    sheet_generation: u64,
    pdf_page: usize,
    pdf_result: Option<Result<PathBuf, String>>,
    sheet_index: usize,
    sheet: Option<Result<Sheet, String>>,
}

impl Default for PreviewSession {
    fn default() -> Self {
        Self {
            target: None,
            line: None,
            state: State::Empty,
            generation: 0,
            pdf_generation: 0,
            sheet_generation: 0,
            pdf_page: 1,
            pdf_result: None,
            sheet_index: 0,
            sheet: None,
        }
    }
}

impl PreviewSession {
    /// Current preview data or loading/failure state.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Search-hit line to reveal without changing the target.
    pub fn line(&self) -> Option<u64> {
        self.line
    }

    /// Selected one-based PDF page.
    pub fn pdf_page(&self) -> usize {
        self.pdf_page
    }

    /// Rendered page path or its error, absent while rendering.
    pub fn pdf_result(&self) -> &Option<Result<PathBuf, String>> {
        &self.pdf_result
    }

    /// Selected zero-based workbook sheet.
    pub fn sheet_index(&self) -> usize {
        self.sheet_index
    }

    /// Sheet sample or error, absent while loading.
    pub fn sheet(&self) -> &Option<Result<Sheet, String>> {
        &self.sheet
    }

    /// Suppresses repeated targets while allowing line-only changes.
    pub fn set_target(
        &mut self,
        path: Option<PathBuf>,
        identity: Option<Location>,
        line: Option<u64>,
    ) -> TargetChange {
        let target =
            (path.is_some() || identity.is_some()).then_some(PreviewTarget { path, identity });
        if target == self.target {
            if line == self.line {
                return TargetChange::Unchanged;
            }
            self.line = line;
            return TargetChange::Reveal;
        }
        self.target = target;
        self.line = line;
        self.reset()
    }

    /// Forces a fresh read of the same target, retaining its line and identity.
    pub fn reload(&mut self) -> TargetChange {
        if self.target.is_none() {
            return TargetChange::Unchanged;
        }
        self.reset()
    }

    fn reset(&mut self) -> TargetChange {
        self.generation = self.generation.wrapping_add(1);
        self.pdf_generation = self.pdf_generation.wrapping_add(1);
        self.sheet_generation = self.sheet_generation.wrapping_add(1);
        self.pdf_page = 1;
        self.pdf_result = None;
        self.sheet_index = 0;
        self.sheet = None;
        let request = self
            .target
            .as_ref()
            .and_then(|target| target.path.clone())
            .map(|path| Request {
                path,
                index: 0,
                generation: self.generation,
            });
        self.state = if request.is_some() {
            State::Loading
        } else {
            State::Empty
        };
        TargetChange::Reset(request)
    }

    /// Accepts a full load only while its request is still current.
    pub fn complete(&mut self, request: &Request, result: Result<Preview, String>) -> bool {
        if request.generation != self.generation || !matches!(self.state, State::Loading) {
            return false;
        }
        self.state = match result {
            Ok(preview) => {
                if let Content::Spreadsheet { first, .. } = &preview.content {
                    self.sheet = Some(Ok(Sheet::clone(first)));
                }
                State::Ready(preview)
            }
            Err(error) => State::Failed(error),
        };
        true
    }

    /// Schedules the selected PDF page, invalidating earlier renders.
    pub fn render_pdf(&mut self) -> Option<Request> {
        let State::Ready(preview) = &self.state else {
            return None;
        };
        let Content::Pdf { path, .. } = &preview.content else {
            return None;
        };
        self.pdf_generation = self.pdf_generation.wrapping_add(1);
        self.pdf_result = None;
        Some(Request {
            path: path.clone(),
            index: self.pdf_page,
            generation: self.pdf_generation,
        })
    }

    /// Moves within PDF bounds; a boundary press schedules no work.
    pub fn change_pdf_page(&mut self, delta: isize) -> Option<Request> {
        let State::Ready(preview) = &self.state else {
            return None;
        };
        let Content::Pdf { pages, .. } = &preview.content else {
            return None;
        };
        let page = self
            .pdf_page
            .saturating_add_signed(delta)
            .clamp(1, (*pages).max(1));
        if page == self.pdf_page {
            return None;
        }
        self.pdf_page = page;
        self.render_pdf()
    }

    /// Accepts a render only for the current page and request generation.
    pub fn complete_pdf(&mut self, request: &Request, result: Result<PathBuf, String>) -> bool {
        if request.generation != self.pdf_generation || request.index != self.pdf_page {
            return false;
        }
        self.pdf_result = Some(result);
        true
    }

    /// Moves within workbook bounds and invalidates earlier sheet reads.
    pub fn change_sheet(&mut self, delta: isize) -> Option<Request> {
        let State::Ready(preview) = &self.state else {
            return None;
        };
        let Content::Spreadsheet { path, sheets, .. } = &preview.content else {
            return None;
        };
        let index = self
            .sheet_index
            .saturating_add_signed(delta)
            .min(sheets.len().saturating_sub(1));
        if index == self.sheet_index {
            return None;
        }
        self.sheet_index = index;
        self.sheet_generation = self.sheet_generation.wrapping_add(1);
        self.sheet = None;
        Some(Request {
            path: path.clone(),
            index,
            generation: self.sheet_generation,
        })
    }

    /// Accepts a sheet read only for the current selection and generation.
    pub fn complete_sheet(&mut self, request: &Request, result: Result<Sheet, String>) -> bool {
        if request.generation != self.sheet_generation || request.index != self.sheet_index {
            return false;
        }
        self.sheet = Some(result);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{PreviewSession, PreviewTarget, Request, State, TargetChange};
    use crate::location::Location as TypedLocation;
    use crate::location::RemoteLocation;
    use crate::preview::{Content, Preview, Sheet};
    use std::path::PathBuf;

    fn request(change: TargetChange) -> Request {
        let TargetChange::Reset(Some(request)) = change else {
            panic!("expected a full load");
        };
        request
    }

    fn select(session: &mut PreviewSession, name: &str) -> Request {
        request(session.set_target(Some(PathBuf::from(name)), None, Some(12)))
    }

    fn preview(content: Content) -> Preview {
        Preview {
            path: PathBuf::from("sample"),
            name: "sample".into(),
            size: 0,
            kind: "Test",
            content,
            link: Default::default(),
            resolved_link: None,
        }
    }

    fn pdf() -> Preview {
        preview(Content::Pdf {
            path: PathBuf::from("sample.pdf"),
            pages: 3,
            metadata: Vec::new(),
        })
    }

    fn workbook() -> Preview {
        preview(Content::Spreadsheet {
            path: PathBuf::from("sample.xlsx"),
            sheets: vec!["First".into(), "Second".into(), "Third".into()],
            first: Box::default(),
        })
    }

    #[test]
    fn repeated_target_and_line_changes_do_not_reload() {
        let mut session = PreviewSession::default();
        let load = select(&mut session, "text");
        assert!(matches!(
            select_change(&mut session),
            TargetChange::Unchanged
        ));
        assert!(matches!(
            session.set_target(Some("text".into()), None, Some(40)),
            TargetChange::Reveal
        ));
        assert_eq!(session.line(), Some(40));
        assert!(session.complete(
            &load,
            Ok(preview(Content::Text {
                text: "hello".into(),
                language: None,
                truncated: false,
            }))
        ));
        assert!(matches!(
            session.set_target(Some("text".into()), None, None),
            TargetChange::Reveal
        ));
        assert_eq!(session.line(), None);
        assert!(matches!(session.state(), State::Ready(_)));
    }

    fn select_change(session: &mut PreviewSession) -> TargetChange {
        session.set_target(Some("text".into()), None, Some(12))
    }

    #[test]
    fn reload_preserves_identity_and_line_and_rejects_old_loads() {
        let mut session = PreviewSession::default();
        let identity = TypedLocation::Remote(RemoteLocation::root("drive").unwrap());
        let old =
            request(session.set_target(Some("cache".into()), Some(identity.clone()), Some(9)));
        let reload = request(session.reload());
        assert_eq!(session.line(), Some(9));
        assert_eq!(session.target.as_ref().unwrap().identity, Some(identity));
        assert!(!session.complete(&old, Err("obsolete".into())));
        assert!(session.complete(&reload, Err("failed".into())));
        assert!(matches!(session.state(), State::Failed(error) if error == "failed"));
        assert!(!session.complete(&reload, Ok(pdf())));
        let retry = request(session.reload());
        assert!(session.complete(&retry, Ok(pdf())));
    }

    #[test]
    fn target_changes_and_clearing_reject_obsolete_completions() {
        let mut session = PreviewSession::default();
        let first = select(&mut session, "first");
        let second = select(&mut session, "second");
        assert!(!session.complete(&first, Ok(pdf())));
        assert!(matches!(session.state(), State::Loading));
        assert!(matches!(
            session.set_target(None, None, None),
            TargetChange::Reset(None)
        ));
        assert!(!session.complete(&second, Err("obsolete".into())));
        assert!(matches!(session.state(), State::Empty));
        assert!(matches!(session.reload(), TargetChange::Unchanged));
    }

    #[test]
    fn typed_identity_changes_reset_even_with_the_same_cache_path() {
        let mut session = PreviewSession::default();
        let first = TypedLocation::Remote(RemoteLocation::root("first").unwrap());
        let second = TypedLocation::Remote(RemoteLocation::root("second").unwrap());
        let old = request(session.set_target(Some("cache".into()), Some(first), None));
        let new = request(session.set_target(Some("cache".into()), Some(second.clone()), None));
        assert!(!session.complete(&old, Ok(pdf())));
        assert!(session.complete(&new, Ok(pdf())));
        assert!(matches!(
            session.set_target(None, Some(second), None),
            TargetChange::Reset(None)
        ));
        assert!(matches!(session.state(), State::Empty));
    }

    #[test]
    fn pdf_bounds_errors_and_stale_renders_are_model_decisions() {
        let mut session = PreviewSession::default();
        assert!(session.change_pdf_page(1).is_none());
        let load = select(&mut session, "pdf");
        assert!(session.complete(&load, Ok(pdf())));
        let first = session.render_pdf().unwrap();
        assert!(session.change_pdf_page(-1).is_none());
        let last = session.change_pdf_page(isize::MAX).unwrap();
        assert_eq!(last.index, 3);
        assert!(session.change_pdf_page(1).is_none());
        assert!(!session.complete_pdf(&first, Ok("old.png".into())));
        assert!(session.complete_pdf(&last, Err("render failed".into())));
        assert!(matches!(session.pdf_result(), Some(Err(error)) if error == "render failed"));
        let back = session.change_pdf_page(isize::MIN).unwrap();
        assert_eq!(back.index, 1);
        assert!(session.pdf_result().is_none());
        assert!(!session.complete_pdf(&first, Ok("old.png".into())));
        assert!(session.complete_pdf(&back, Ok("new.png".into())));
        let _ = session.reload();
        assert_eq!(session.pdf_page(), 1);
        assert!(session.pdf_result().is_none());
        assert!(!session.complete_pdf(&back, Err("obsolete".into())));
    }

    #[test]
    fn workbook_bounds_samples_and_stale_reads_are_model_decisions() {
        let mut session = PreviewSession::default();
        assert!(session.change_sheet(1).is_none());
        let load = select(&mut session, "workbook");
        assert!(session.complete(&load, Ok(workbook())));
        assert_eq!(session.sheet(), &Some(Ok(Sheet::default())));
        assert!(session.change_sheet(-1).is_none());
        let second = session.change_sheet(1).unwrap();
        assert!(session.sheet().is_none());
        let last = session.change_sheet(isize::MAX).unwrap();
        assert_eq!(last.index, 2);
        assert!(session.change_sheet(1).is_none());
        assert!(!session.complete_sheet(&second, Ok(Sheet::default())));
        assert!(session.complete_sheet(&last, Err("sheet failed".into())));
        let back = session.change_sheet(isize::MIN).unwrap();
        assert_eq!(back.index, 0);
        assert!(session.complete_sheet(&back, Ok(Sheet::default())));
        let next = select(&mut session, "other");
        assert_eq!(session.sheet_index(), 0);
        assert!(session.sheet().is_none());
        assert!(!session.complete_sheet(&back, Err("obsolete".into())));
        assert!(session.complete(&next, Ok(pdf())));
        assert!(session.change_sheet(1).is_none());
    }

    #[test]
    fn materialized_preview_identity_is_not_its_cache_path() {
        let path = PathBuf::from("/cache/fyle/remote/gdrive/report.docx");
        let first = PreviewTarget {
            path: Some(path.clone()),
            identity: Some(TypedLocation::Remote(
                RemoteLocation::root("gdrive")
                    .unwrap()
                    .child("report")
                    .unwrap(),
            )),
        };
        let other = PreviewTarget {
            path: Some(path),
            identity: Some(TypedLocation::Remote(
                RemoteLocation::root("onedrive")
                    .unwrap()
                    .child("report")
                    .unwrap(),
            )),
        };
        assert_ne!(first, other);
        let unavailable = PreviewTarget {
            path: None,
            identity: first.identity.clone(),
        };
        assert_ne!(first, unavailable);
    }
}
