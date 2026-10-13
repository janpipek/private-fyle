//! UI-independent workspace state: visible panels, inspector perspective,
//! visible panel roles, retained inspector perspective, and feasible layouts.

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

/// State shared by the layout chooser and whichever inspector view is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    mode: LayoutMode,
    last_inspector_mode: LayoutMode,
    perspective: Perspective,
    active: PanelSide,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            mode: LayoutMode::TwoPanels,
            last_inspector_mode: LayoutMode::TwoPanelsInspector,
            perspective: Perspective::Preview,
            active: PanelSide::Left,
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

    /// Chooses one of the three layouts without discarding either panel's selection.
    pub fn set_mode(&mut self, mode: LayoutMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        if mode.has_inspector() {
            self.last_inspector_mode = mode;
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

    /// Applies F3 behavior and returns whether the persisted layout changed.
    pub fn toggle_preview(&mut self) -> bool {
        if self.perspective == Perspective::Preview && self.mode.has_inspector() {
            self.toggle_inspector();
            return true;
        }
        let opened = !self.mode.has_inspector();
        if opened {
            self.toggle_inspector();
        }
        self.set_perspective(Perspective::Preview);
        opened
    }

    /// Opens the inspector on Properties and reports whether the layout opened.
    pub fn show_properties(&mut self) -> bool {
        let opened = !self.mode.has_inspector();
        if opened {
            self.toggle_inspector();
        }
        self.set_perspective(Perspective::Properties);
        opened
    }

    /// Selects which inspector tab is visible.
    pub fn set_perspective(&mut self, perspective: Perspective) {
        if self.perspective != perspective {
            self.perspective = perspective;
        }
    }

    /// Makes `panel` the command source, following its saved selection.
    pub fn set_active_panel(&mut self, panel: PanelSide) {
        if self.active != panel {
            self.active = panel;
        }
    }

    /// Switches command source; in the one-panel layout this also switches the visible folder.
    pub fn switch_panel(&mut self) {
        self.set_active_panel(self.active.other());
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
    use super::{Fit, LayoutMode, MinSizes, PanelSide, Perspective, Viewport, Workspace, fit};

    #[test]
    fn layouts_keep_both_panel_roles_and_active_side() {
        let mut workspace = Workspace::default();
        assert_eq!(workspace.visible_panels(), [true, true]);

        workspace.set_mode(LayoutMode::OnePanelInspector);
        assert_eq!(workspace.visible_panels(), [true, false]);
        assert_eq!(workspace.destination_panel(), PanelSide::Right);

        workspace.switch_panel();
        assert_eq!(workspace.visible_panels(), [false, true]);
        assert_eq!(workspace.destination_panel(), PanelSide::Left);

        workspace.set_mode(LayoutMode::TwoPanelsInspector);
        assert_eq!(workspace.visible_panels(), [true, true]);
        workspace.set_mode(LayoutMode::TwoPanels);
        workspace.set_mode(LayoutMode::OnePanelInspector);
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
    fn f3_selects_view_from_properties_and_only_closes_active_view() {
        let mut workspace = Workspace::default();
        assert!(workspace.show_properties());
        assert_eq!(workspace.mode(), LayoutMode::TwoPanelsInspector);
        assert_eq!(workspace.perspective(), Perspective::Properties);

        assert!(!workspace.toggle_preview());
        assert_eq!(workspace.mode(), LayoutMode::TwoPanelsInspector);
        assert_eq!(workspace.perspective(), Perspective::Preview);

        assert!(workspace.toggle_preview());
        assert_eq!(workspace.mode(), LayoutMode::TwoPanels);
        assert!(workspace.toggle_preview());
        assert_eq!(workspace.mode(), LayoutMode::TwoPanelsInspector);
        assert_eq!(workspace.perspective(), Perspective::Preview);
    }

    #[test]
    fn layout_changes_keep_the_selected_perspective() {
        let mut workspace = Workspace::default();
        workspace.show_properties();
        workspace.set_mode(LayoutMode::OnePanelInspector);
        assert_eq!(workspace.perspective(), Perspective::Properties);
        workspace.set_mode(LayoutMode::TwoPanels);
        assert_eq!(workspace.perspective(), Perspective::Properties);
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
