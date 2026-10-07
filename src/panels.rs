//! Commands that relate the two panels to each other, as UI-independent
//! decisions: where "Go to Other Panel's Folder" leads.

use crate::location::PanelTarget;

/// Why the other panel's folder cannot be followed.
pub const OTHER_IS_PLACES: &str = "The other panel shows Places";

/// Where the active panel goes to show the other panel's folder.
///
/// `shown` is what the other panel's content is addressed by
/// ([`crate::model::Folder::target`]): its typed local, remote or archive
/// location, or the search root while it shows search results. `pending` is
/// the destination of a navigation still loading there. It wins over `shown`,
/// because the user has already chosen it and `shown` is about to be replaced;
/// if that load fails, the active panel gets the same error as the other one
/// would. Places have no folder to follow.
pub fn other_panel_target(
    shown: &PanelTarget,
    pending: Option<&PanelTarget>,
) -> Result<PanelTarget, &'static str> {
    match pending.unwrap_or(shown) {
        PanelTarget::Places => Err(OTHER_IS_PLACES),
        target => Ok(target.clone()),
    }
}

/// What the window is doing that decides whether the panels can be swapped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Activity {
    /// The F4 editor covers the inactive panel.
    pub editor_open: bool,
    /// The F3 quick view covers the inactive panel.
    pub preview_open: bool,
    /// A navigation, archive read or remote reload is in flight.
    pub loading: bool,
    /// A search or a folder size scan is running.
    pub scanning: bool,
}

/// Why the panels cannot be swapped now, if they cannot.
///
/// Background work is bound to a panel by its position, so rather than
/// re-pointing it, swapping waits until it is finished or cancelled (Esc).
/// Metadata and thumbnails are not affected: they are keyed by the listing
/// generation, which [`crate::model::Folder::swap_with`] renews. The editor
/// and the quick view cover the inactive panel, so the swap would be
/// invisible on one side; close them first.
pub fn swap_blocked(activity: &Activity) -> Option<&'static str> {
    if activity.editor_open {
        Some("Close the editor first")
    } else if activity.preview_open {
        Some("Close the preview (F3) first")
    } else if activity.loading {
        Some("A folder is still loading (Esc cancels)")
    } else if activity.scanning {
        Some("A search or size scan is running (Esc stops it)")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Activity, OTHER_IS_PLACES, other_panel_target, swap_blocked};
    use crate::location::{Location, PanelTarget};
    use crate::model::Folder;
    use crate::model::test_support::{fixture, loaded};
    use crate::remote;

    fn local(path: &str) -> PanelTarget {
        PanelTarget::Location(Location::local(PathBuf::from(path)).unwrap())
    }

    #[test]
    fn follows_a_local_folder() {
        let dir = fixture();
        let folder = loaded(&dir);
        assert_eq!(
            other_panel_target(&folder.target(), None),
            Ok(PanelTarget::Location(
                Location::local(dir.path().to_path_buf()).unwrap()
            ))
        );
    }

    #[test]
    fn keeps_a_remote_location_typed() {
        let remote = Location::Remote(remote::Location::root("proton").unwrap());
        let folder = Folder::new(remote.panel_path()).unwrap();
        let target = other_panel_target(&folder.target(), None).unwrap();
        assert_eq!(target, PanelTarget::Location(remote));
    }

    #[test]
    fn keeps_an_archive_location_typed() {
        let archive =
            Location::archive(Location::local("/data/a.zip".into()).unwrap(), "docs").unwrap();
        let shown = PanelTarget::Location(archive.clone());
        assert_eq!(
            other_panel_target(&shown, None),
            Ok(PanelTarget::Location(archive))
        );
    }

    #[test]
    fn search_results_lead_to_the_search_root() {
        let dir = fixture();
        let mut folder = loaded(&dir);
        folder.show_results(dir.path().join("sub"), "found".into());
        assert_eq!(
            other_panel_target(&folder.target(), None),
            Ok(local(&dir.path().join("sub").to_string_lossy()))
        );
    }

    #[test]
    fn places_have_no_folder() {
        assert_eq!(
            other_panel_target(&PanelTarget::Places, None),
            Err(OTHER_IS_PLACES)
        );
    }

    #[test]
    fn a_pending_load_wins_over_the_shown_folder() {
        let shown = local("/old");
        let pending = local("/new");
        assert_eq!(
            other_panel_target(&shown, Some(&pending)),
            Ok(pending.clone())
        );
        // Also when the panel shows Places now but is heading to a folder,
        // and the other way round.
        assert_eq!(
            other_panel_target(&PanelTarget::Places, Some(&pending)),
            Ok(pending)
        );
        assert_eq!(
            other_panel_target(&shown, Some(&PanelTarget::Places)),
            Err(OTHER_IS_PLACES)
        );
    }

    #[test]
    fn swapping_waits_for_work_and_covering_panes() {
        assert_eq!(swap_blocked(&Activity::default()), None);
        for activity in [
            Activity {
                editor_open: true,
                ..Default::default()
            },
            Activity {
                preview_open: true,
                ..Default::default()
            },
            Activity {
                loading: true,
                ..Default::default()
            },
            Activity {
                scanning: true,
                ..Default::default()
            },
        ] {
            assert!(swap_blocked(&activity).is_some(), "{activity:?}");
        }
    }
}
