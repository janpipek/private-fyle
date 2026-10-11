//! UI-independent workspace state: visible panels, inspector perspective,
//! selection-following requests, and whether a viewport can fit each layout.

use crate::location::Location;
use serde::Deserialize;

/// Which folder panel is the source of commands and inspector selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelSide {
    /// The left folder panel.
    #[default]
    Left,
    /// The right folder panel.
    Right,
}

impl PanelSide {
    /// The panel used as the destination of two-panel file operations.
    pub fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }
}

/// Which folder panels and inspector are visible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutMode {
    /// The current two-folder workspace.
    #[default]
    TwoPanels,
    /// Both folders plus one inspector.
    TwoPanelsInspector,
    /// The active folder plus one inspector; the other folder remains a destination.
    OnePanelInspector,
}

impl LayoutMode {
    /// The value stored in configuration.
    pub fn id(self) -> &'static str {
        match self {
            Self::TwoPanels => "two_panels",
            Self::TwoPanelsInspector => "two_panels_inspector",
            Self::OnePanelInspector => "one_panel_inspector",
        }
    }

    /// Whether this layout has an inspector slot.
    pub fn has_inspector(self) -> bool {
        self != Self::TwoPanels
    }

    fn panel_count(self) -> usize {
        match self {
            Self::OnePanelInspector => 1,
            Self::TwoPanels | Self::TwoPanelsInspector => 2,
        }
    }
}

/// Which kind of content the inspector asks to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Perspective {
    /// Content quick view.
    #[default]
    Preview,
    /// Filesystem and format metadata.
    Properties,
}

/// A load request identified by its selected item and monotonically increasing generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// Rejects a completion from an earlier selection or perspective.
    pub generation: u64,
    /// The folder panel whose selection produced this request.
    pub panel: PanelSide,
    /// The selected item, retaining its local, remote, or archive identity.
    pub location: Location,
    /// The content this request asks for.
    pub perspective: Perspective,
}

/// Whether the active inspector request has finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InspectorLoad {
    /// There is no inspector or no selected item to inspect.
    Empty,
    /// The view may start this request in the background.
    Pending(Request),
    /// The current request has completed successfully.
    Ready(Request),
    /// The current request failed; the view owns the error text.
    Failed(Request),
}

/// State shared by the layout chooser and whichever inspector view is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    mode: LayoutMode,
    last_inspector_mode: LayoutMode,
    perspective: Perspective,
    active: PanelSide,
    selections: [Option<Location>; 2],
    generation: u64,
    load: InspectorLoad,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            mode: LayoutMode::TwoPanels,
            last_inspector_mode: LayoutMode::TwoPanelsInspector,
            perspective: Perspective::Preview,
            active: PanelSide::Left,
            selections: [None, None],
            generation: 0,
            load: InspectorLoad::Empty,
        }
    }
}

impl Workspace {
    /// The chosen arrangement, independent of window size.
    pub fn mode(&self) -> LayoutMode {
        self.mode
    }

    /// The selected inspector perspective, retained when the inspector closes.
    pub fn perspective(&self) -> Perspective {
        self.perspective
    }

    /// The panel that owns commands and the inspector's followed selection.
    pub fn active_panel(&self) -> PanelSide {
        self.active
    }

    /// The other panel, retained as a transfer destination even when hidden.
    pub fn destination_panel(&self) -> PanelSide {
        self.active.other()
    }

    /// Which of the left and right folder panels the layout shows.
    pub fn visible_panels(&self) -> [bool; 2] {
        match self.mode {
            LayoutMode::OnePanelInspector => [
                self.active == PanelSide::Left,
                self.active == PanelSide::Right,
            ],
            LayoutMode::TwoPanels | LayoutMode::TwoPanelsInspector => [true, true],
        }
    }

    /// The inspector's current load state.
    pub fn inspector_load(&self) -> &InspectorLoad {
        &self.load
    }

    /// Chooses one of the three layouts without discarding either panel's selection.
    pub fn set_mode(&mut self, mode: LayoutMode) {
        if self.mode == mode {
            return;
        }
        let inspector_changed = self.mode.has_inspector() != mode.has_inspector();
        self.mode = mode;
        if mode.has_inspector() {
            self.last_inspector_mode = mode;
        }
        if inspector_changed {
            self.restart_request();
        }
    }

    /// F3 closes the inspector or restores its last one- or two-panel layout.
    pub fn toggle_inspector(&mut self) {
        if self.mode.has_inspector() {
            self.set_mode(LayoutMode::TwoPanels);
        } else {
            self.set_mode(self.last_inspector_mode);
        }
    }

    /// Changes the inspector content; an old perspective's result becomes stale.
    pub fn set_perspective(&mut self, perspective: Perspective) {
        if self.perspective != perspective {
            self.perspective = perspective;
            self.restart_request();
        }
    }

    /// Makes `panel` the command source, following its saved selection.
    pub fn set_active_panel(&mut self, panel: PanelSide) {
        if self.active != panel {
            self.active = panel;
            self.restart_request();
        }
    }

    /// Switches command source; in the one-panel layout this also switches the visible folder.
    pub fn switch_panel(&mut self) {
        self.set_active_panel(self.active.other());
    }

    /// Remembers a panel's current item; `None` covers `..` and empty listings.
    pub fn set_selection(&mut self, panel: PanelSide, location: Option<Location>) {
        let slot = &mut self.selections[panel.index()];
        if *slot == location {
            return;
        }
        *slot = location;
        if panel == self.active {
            self.restart_request();
        }
    }

    /// Starts a fresh load for the active item after its contents change in place.
    pub fn refresh_inspector(&mut self) {
        self.restart_request();
    }

    /// Accepts only a completion for the current pending request.
    pub fn complete_request(&mut self, request: &Request, succeeded: bool) -> bool {
        if !matches!(&self.load, InspectorLoad::Pending(current) if current == request) {
            return false;
        }
        self.load = if succeeded {
            InspectorLoad::Ready(request.clone())
        } else {
            InspectorLoad::Failed(request.clone())
        };
        true
    }

    fn restart_request(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.load = if self.mode.has_inspector() {
            self.selections[self.active.index()]
                .clone()
                .map(|location| {
                    InspectorLoad::Pending(Request {
                        generation: self.generation,
                        panel: self.active,
                        location,
                        perspective: self.perspective,
                    })
                })
                .unwrap_or(InspectorLoad::Empty)
        } else {
            InspectorLoad::Empty
        };
    }
}

/// Which dock positions can hold every pane at the supplied minimum sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fit {
    /// The inspector can sit beside the folder panels.
    pub right: bool,
    /// The inspector can sit below the folder panels.
    pub bottom: bool,
}

/// Available logical space and a scale for minimum usable pane sizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// Available width in logical pixels.
    pub width: f32,
    /// Available height in logical pixels.
    pub height: f32,
    /// Multiplier for the minimum usable sizes, including UI zoom.
    pub scale: f32,
}

/// Minimum sizes supplied by the caller; their values remain a UI decision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinSizes {
    /// Least width of each folder panel.
    pub panel_width: f32,
    /// Least height of each folder panel.
    pub panel_height: f32,
    /// Least width of the inspector when docked on the right.
    pub inspector_width: f32,
    /// Least height of the inspector when docked below.
    pub inspector_height: f32,
    /// Space between adjacent panes.
    pub gap: f32,
}

/// Reports feasible dock positions without choosing between them. `None`
/// means no arrangement can meet the supplied minimum sizes.
pub fn fit(mode: LayoutMode, viewport: Viewport, minimum: MinSizes) -> Option<Fit> {
    let values = [
        viewport.width,
        viewport.height,
        viewport.scale,
        minimum.panel_width,
        minimum.panel_height,
        minimum.inspector_width,
        minimum.inspector_height,
        minimum.gap,
    ];
    if values
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
        || viewport.scale == 0.0
    {
        return None;
    }
    let scale = viewport.scale;
    let gap = minimum.gap * scale;
    let panel_width = minimum.panel_width * scale;
    let panel_height = minimum.panel_height * scale;
    let inspector_width = minimum.inspector_width * scale;
    let inspector_height = minimum.inspector_height * scale;
    let folders_width =
        panel_width * mode.panel_count() as f32 + gap * (mode.panel_count() - 1) as f32;

    if !mode.has_inspector() {
        return (viewport.width >= folders_width && viewport.height >= panel_height).then_some(
            Fit {
                right: false,
                bottom: false,
            },
        );
    }
    let right = viewport.width >= folders_width + gap + inspector_width
        && viewport.height >= panel_height.max(inspector_height);
    let bottom = viewport.width >= folders_width.max(inspector_width)
        && viewport.height >= panel_height + gap + inspector_height;
    (right || bottom).then_some(Fit { right, bottom })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        Fit, InspectorLoad, LayoutMode, MinSizes, PanelSide, Perspective, Viewport, Workspace, fit,
    };
    use crate::location::Location;

    fn local(path: &str) -> Location {
        #[cfg(unix)]
        let path = PathBuf::from(path);
        #[cfg(windows)]
        let path = PathBuf::from(format!("C:{}", path.replace('/', "\\")));
        Location::local(path).expect("test paths are absolute")
    }

    fn pending(workspace: &Workspace) -> super::Request {
        match workspace.inspector_load() {
            InspectorLoad::Pending(request) => request.clone(),
            state => panic!("expected pending inspector request, got {state:?}"),
        }
    }

    #[test]
    fn layouts_keep_both_panel_roles_and_selections() {
        let mut workspace = Workspace::default();
        workspace.set_selection(PanelSide::Left, Some(local("/left/file")));
        workspace.set_selection(PanelSide::Right, Some(local("/right/file")));
        assert_eq!(workspace.visible_panels(), [true, true]);
        assert_eq!(workspace.inspector_load(), &InspectorLoad::Empty);

        workspace.set_mode(LayoutMode::OnePanelInspector);
        assert_eq!(workspace.visible_panels(), [true, false]);
        assert_eq!(workspace.destination_panel(), PanelSide::Right);
        assert_eq!(pending(&workspace).location, local("/left/file"));

        workspace.switch_panel();
        assert_eq!(workspace.visible_panels(), [false, true]);
        assert_eq!(workspace.destination_panel(), PanelSide::Left);
        assert_eq!(pending(&workspace).location, local("/right/file"));

        workspace.set_mode(LayoutMode::TwoPanelsInspector);
        assert_eq!(workspace.visible_panels(), [true, true]);
        assert_eq!(pending(&workspace).location, local("/right/file"));
        workspace.set_mode(LayoutMode::TwoPanels);
        assert_eq!(workspace.inspector_load(), &InspectorLoad::Empty);
        workspace.set_mode(LayoutMode::OnePanelInspector);
        assert_eq!(pending(&workspace).location, local("/right/file"));
    }

    #[test]
    fn f3_restores_the_last_inspector_layout() {
        let mut workspace = Workspace::default();
        workspace.toggle_inspector();
        assert_eq!(workspace.mode(), LayoutMode::TwoPanelsInspector);
        workspace.set_mode(LayoutMode::OnePanelInspector);
        workspace.toggle_inspector();
        assert_eq!(workspace.mode(), LayoutMode::TwoPanels);
        workspace.toggle_inspector();
        assert_eq!(workspace.mode(), LayoutMode::OnePanelInspector);
    }

    #[test]
    fn perspective_and_selection_changes_reject_late_results() {
        let mut workspace = Workspace::default();
        workspace.set_selection(PanelSide::Left, Some(local("/left/first")));
        workspace.set_mode(LayoutMode::TwoPanelsInspector);
        let first = pending(&workspace);
        workspace.set_selection(PanelSide::Right, Some(local("/right/hidden")));
        assert_eq!(pending(&workspace), first);

        workspace.set_selection(PanelSide::Left, Some(local("/left/second")));
        let second = pending(&workspace);
        assert!(!workspace.complete_request(&first, true));
        workspace.set_perspective(Perspective::Properties);
        let properties = pending(&workspace);
        assert_eq!(properties.perspective, Perspective::Properties);
        assert!(!workspace.complete_request(&second, false));
        assert!(workspace.complete_request(&properties, true));
        assert_eq!(
            workspace.inspector_load(),
            &InspectorLoad::Ready(properties)
        );
    }

    #[test]
    fn closing_or_refreshing_invalidates_pending_work() {
        let mut workspace = Workspace::default();
        workspace.set_selection(PanelSide::Left, Some(local("/left/file")));
        workspace.set_mode(LayoutMode::OnePanelInspector);
        let before_refresh = pending(&workspace);
        workspace.refresh_inspector();
        let after_refresh = pending(&workspace);
        assert_ne!(before_refresh.generation, after_refresh.generation);
        assert!(!workspace.complete_request(&before_refresh, true));
        workspace.set_selection(PanelSide::Left, None);
        assert_eq!(workspace.inspector_load(), &InspectorLoad::Empty);
        assert!(!workspace.complete_request(&after_refresh, true));
    }

    fn minimum() -> MinSizes {
        MinSizes {
            panel_width: 320.0,
            panel_height: 240.0,
            inspector_width: 300.0,
            inspector_height: 200.0,
            gap: 8.0,
        }
    }

    fn viewport(width: f32, height: f32, scale: f32) -> Viewport {
        Viewport {
            width,
            height,
            scale,
        }
    }

    #[test]
    fn resize_reports_docks_only_when_all_panes_fit() {
        let mode = LayoutMode::TwoPanelsInspector;
        assert_eq!(
            fit(mode, viewport(1280.0, 800.0, 1.0), minimum()),
            Some(Fit {
                right: true,
                bottom: true
            })
        );
        assert_eq!(
            fit(mode, viewport(800.0, 800.0, 1.0), minimum()),
            Some(Fit {
                right: false,
                bottom: true
            })
        );
        assert!(fit(mode, viewport(600.0, 800.0, 1.0), minimum()).is_none());
        assert!(fit(mode, viewport(800.0, 400.0, 1.0), minimum()).is_none());
        assert_eq!(
            fit(mode, viewport(1280.0, 800.0, 1.5), minimum()),
            Some(Fit {
                right: false,
                bottom: true
            })
        );
    }

    #[test]
    fn one_panel_inspector_can_fit_where_three_panes_cannot() {
        let size = viewport(640.0, 500.0, 1.0);
        assert!(fit(LayoutMode::TwoPanelsInspector, size, minimum()).is_none());
        assert_eq!(
            fit(LayoutMode::OnePanelInspector, size, minimum()),
            Some(Fit {
                right: true,
                bottom: true
            })
        );
        assert!(fit(LayoutMode::TwoPanels, size, minimum()).is_none());
        assert!(
            fit(
                LayoutMode::TwoPanels,
                viewport(f32::NAN, 500.0, 1.0),
                minimum()
            )
            .is_none()
        );
    }
}
