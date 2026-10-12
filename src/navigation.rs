//! Per-panel navigation request lifecycle, independent of GUI tasks and listing I/O.

use crate::location::PanelTarget;

/// Identifies a navigation request within the panel that issued it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestToken(u64);

/// Owns request freshness and the destination being loaded for one panel position.
#[derive(Default)]
pub struct PanelNavigation {
    generation: u64,
    pending_target: Option<PanelTarget>,
}

impl PanelNavigation {
    /// Starts a newer request, even when its destination matches the previous one.
    pub fn begin(&mut self, target: PanelTarget) -> RequestToken {
        self.generation = self.generation.wrapping_add(1);
        self.pending_target = Some(target);
        RequestToken(self.generation)
    }

    /// Clears a current pending request, for either a successful or failed listing.
    /// Obsolete completions leave the newer destination untouched.
    pub fn complete(&mut self, token: RequestToken) -> bool {
        if !self.is_pending() || token.0 != self.generation {
            return false;
        }
        self.pending_target = None;
        true
    }

    /// Invalidates pending work and clears its destination; returns whether any existed.
    pub fn cancel(&mut self) -> bool {
        if self.pending_target.take().is_none() {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        true
    }

    /// Whether a listing is still being loaded for this panel.
    pub fn is_pending(&self) -> bool {
        self.pending_target.is_some()
    }

    /// Destination preferred over the displayed folder when following this panel.
    pub fn pending_target(&self) -> Option<&PanelTarget> {
        self.pending_target.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::{PanelNavigation, RequestToken};
    use crate::breadcrumbs::NavigationGuard;
    use crate::location::{Location, PanelTarget, RemoteLocation};
    use crate::panels::{Activity, other_panel_target, swap_blocked};
    use std::path::PathBuf;

    fn local(name: &str) -> PanelTarget {
        #[cfg(unix)]
        let root = PathBuf::from("/navigation-test");
        #[cfg(windows)]
        let root = PathBuf::from("C:\\navigation-test");
        PanelTarget::Location(Location::local(root.join(name)).unwrap())
    }

    #[test]
    fn idle_state_has_no_destination_or_work_to_cancel() {
        let mut navigation = PanelNavigation::default();
        assert!(!navigation.is_pending());
        assert_eq!(navigation.pending_target(), None);
        assert!(!navigation.cancel());
        assert!(!navigation.complete(RequestToken(0)));
    }

    #[test]
    fn current_completion_clears_pending_state_once() {
        let mut navigation = PanelNavigation::default();
        let target = local("folder");
        let token = navigation.begin(target.clone());
        assert!(navigation.is_pending());
        assert_eq!(navigation.pending_target(), Some(&target));
        assert!(navigation.complete(token));
        assert!(!navigation.is_pending());
        assert_eq!(navigation.pending_target(), None);
        assert!(!navigation.complete(token));
        assert!(!navigation.cancel());
    }

    #[test]
    fn repeating_a_target_still_replaces_the_request() {
        let mut navigation = PanelNavigation::default();
        let target = local("folder");
        let old = navigation.begin(target.clone());
        let new = navigation.begin(target.clone());
        assert_ne!(old, new);
        assert!(!navigation.complete(old));
        assert_eq!(navigation.pending_target(), Some(&target));
        assert!(navigation.complete(new));
    }

    #[test]
    fn returning_to_an_earlier_target_rejects_both_obsolete_requests() {
        let mut navigation = PanelNavigation::default();
        let target = local("a");
        let first = navigation.begin(target.clone());
        let second = navigation.begin(local("b"));
        let current = navigation.begin(target.clone());
        for obsolete in [first, second] {
            assert!(!navigation.complete(obsolete));
            assert!(navigation.is_pending());
            assert_eq!(navigation.pending_target(), Some(&target));
        }
        assert!(navigation.complete(current));
    }

    #[test]
    fn cancellation_rejects_late_results_before_and_after_a_new_request() {
        let mut navigation = PanelNavigation::default();
        let target = local("folder");
        let cancelled = navigation.begin(target.clone());
        assert!(navigation.cancel());
        assert_eq!(navigation.pending_target(), None);
        assert!(!navigation.is_pending());
        assert!(!navigation.complete(cancelled));
        assert!(!navigation.cancel());
        let current = navigation.begin(target.clone());
        assert!(!navigation.complete(cancelled));
        assert_eq!(navigation.pending_target(), Some(&target));
        assert!(navigation.complete(current));
    }

    #[test]
    fn local_remote_archive_and_places_targets_remain_typed() {
        let PanelTarget::Location(source) = local("archive.zip") else {
            panic!("expected a local source");
        };
        let targets = [
            local("spelling/../folder"),
            PanelTarget::Location(Location::Remote(RemoteLocation::root("nas").unwrap())),
            PanelTarget::Location(Location::archive(source, "folder/member").unwrap()),
            PanelTarget::Places,
        ];
        let mut navigation = PanelNavigation::default();
        for target in targets {
            let token = navigation.begin(target.clone());
            assert_eq!(navigation.pending_target(), Some(&target));
            assert!(navigation.complete(token));
        }
    }

    #[test]
    fn panel_states_are_independent_and_feed_existing_panel_decisions() {
        let mut panels = [PanelNavigation::default(), PanelNavigation::default()];
        let shown = local("shown");
        let destination = local("destination");
        let left = panels[0].begin(destination.clone());
        let right = panels[1].begin(PanelTarget::Places);
        assert_eq!(
            other_panel_target(&shown, panels[0].pending_target()),
            Ok(destination)
        );
        assert!(other_panel_target(&shown, panels[1].pending_target()).is_err());
        assert!(
            swap_blocked(&Activity {
                loading: panels.iter().any(PanelNavigation::is_pending),
                scanning: false,
            })
            .is_some()
        );
        assert!(panels[0].complete(left));
        assert!(panels[1].is_pending());
        assert_eq!(
            other_panel_target(&shown, panels[0].pending_target()),
            Ok(shown)
        );
        assert!(panels[1].cancel());
        assert!(!panels[1].complete(right));
        assert!(
            swap_blocked(&Activity {
                loading: panels.iter().any(PanelNavigation::is_pending),
                scanning: false,
            })
            .is_none()
        );
    }

    #[test]
    fn current_completion_clears_state_before_a_changed_folder_guard_rejects_it() {
        let mut navigation = PanelNavigation::default();
        let source = local("source");
        let guard = NavigationGuard::new(source.clone(), 1);
        let request = navigation.begin(local("destination"));
        assert!(navigation.complete(request));
        assert!(!guard.matches(&source, 2));
        assert!(!navigation.is_pending());
        assert_eq!(navigation.pending_target(), None);
    }

    #[test]
    fn wrapping_generation_still_rejects_the_previous_request() {
        let mut navigation = PanelNavigation {
            generation: u64::MAX - 1,
            ..PanelNavigation::default()
        };
        let old = navigation.begin(local("old"));
        let current = navigation.begin(local("current"));
        assert!(!navigation.complete(old));
        assert!(navigation.complete(current));
    }
}
