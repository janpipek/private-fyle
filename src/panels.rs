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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{OTHER_IS_PLACES, other_panel_target};
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
}
