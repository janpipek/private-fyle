//! Synchronous test driver using production listing and folder commit paths.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

use super::Folder;
use crate::entry::Entry;
use crate::listing::contracts::ListingSnapshot;
use crate::listing::{ListingService, RemoteListingCache};
use crate::location::{Location, PanelTarget, RemoteLocation};
use crate::remote::{Capabilities, Control, Listing, Provider, Registry};

/// Result of driving activation in a model test.
#[derive(Debug, PartialEq, Eq)]
pub enum Activation {
    /// A directory or search-hit destination was entered.
    Entered,
    /// A native or remote file would be opened.
    OpenFile(Location),
    /// There is no current row.
    Nothing,
}

type RemoteFixtures = Arc<Mutex<HashMap<RemoteLocation, Vec<Entry>>>>;

struct FixtureProvider {
    name: String,
    listings: RemoteFixtures,
}

impl Provider for FixtureProvider {
    fn name(&self) -> &str {
        &self.name
    }
    fn label(&self) -> &str {
        &self.name
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    fn list(&self, dir: &RemoteLocation, _: &Control) -> io::Result<Listing> {
        let entries = self
            .listings
            .lock()
            .expect("fixture lock is never poisoned")
            .get(dir)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no remote fixture"))?;
        Ok(Listing {
            entries,
            warnings: Vec::new(),
        })
    }
    fn download(&self, _: &RemoteLocation, _: bool, _: &Path, _: &Control) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "fixtures only support listing",
        ))
    }
}

/// Owns a plain Folder and its listing service for synchronous model tests.
pub struct TestFolder {
    folder: Folder,
    service: ListingService,
    fixtures: RemoteFixtures,
    cache: Arc<RemoteListingCache>,
}

impl Deref for TestFolder {
    type Target = Folder;
    fn deref(&self) -> &Folder {
        &self.folder
    }
}

impl DerefMut for TestFolder {
    fn deref_mut(&mut self) -> &mut Folder {
        &mut self.folder
    }
}

impl TestFolder {
    /// Creates an unloaded model and isolated listing fixtures.
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let cache = Arc::default();
        Ok(Self {
            folder: Folder::new(path)?,
            service: ListingService::with_cache(Arc::default(), Arc::clone(&cache)),
            fixtures: Arc::default(),
            cache,
        })
    }

    fn snapshot(
        &self,
        target: PanelTarget,
        preserve_search: bool,
    ) -> Result<ListingSnapshot, String> {
        let request = self
            .folder
            .listing_request(target.clone(), false, preserve_search);
        self.service
            .list(&request, &AtomicBool::new(false))
            .map_err(|error| format!("{}: {error}", target.panel_path().display()))
    }

    /// Refreshes rows using the production service, preserving search and selection.
    pub fn load(&mut self) -> Result<(), String> {
        let snapshot = self.snapshot(self.target(), true)?;
        let selected = self.current().map(|row| row.location.clone());
        self.folder.apply_listing(snapshot, selected);
        Ok(())
    }

    /// Loads with the service's ancestor recovery and reports its first error.
    pub fn load_or_ancestor(&mut self) -> Option<String> {
        let target = self.target();
        let request = self.folder.listing_request(target.clone(), true, true);
        match self
            .service
            .list_with_ancestor_fallback(&request, &AtomicBool::new(false))
        {
            Ok(outcome) => {
                let error = outcome
                    .first_error
                    .map(|error| format!("{}: {error}", target.panel_path().display()));
                let selected = self.current().map(|row| row.location.clone());
                self.folder.apply_listing(outcome.snapshot, selected);
                error
            }
            Err(error) => Some(format!("{}: {error}", target.panel_path().display())),
        }
    }

    /// Explicit refresh invalidates calculated sizes before reading rows.
    pub fn refresh(&mut self) -> Option<String> {
        self.clear_sizes();
        self.load_or_ancestor()
    }

    /// Drives a typed navigation through the production selection and commit rules.
    pub fn navigate_target(&mut self, target: PanelTarget) -> Result<(), String> {
        let snapshot = self.snapshot(target, false)?;
        let selected = self.folder.selection_for_navigation(&snapshot.target);
        self.folder.commit_navigation_listing(snapshot, selected);
        Ok(())
    }

    /// Navigates to a local, remote or archive address.
    pub fn navigate_location(&mut self, location: Location) -> Result<(), String> {
        self.navigate_target(PanelTarget::Location(location))
    }

    /// Parses a test panel spelling before typed navigation.
    pub fn navigate(&mut self, path: PathBuf) -> Result<(), String> {
        let target = PanelTarget::from_panel_path(&path, self.archive())
            .map_err(|error| error.to_string())?;
        self.navigate_target(target)
    }

    /// Drives the model's parent destination through the listing service.
    pub fn go_up(&mut self) -> Result<(), String> {
        match self.parent_target() {
            Some(target) => self.navigate_target(target),
            None => Ok(()),
        }
    }

    /// Returns the typed parent as a panel spelling for assertions.
    pub fn parent_location(&self) -> Option<PathBuf> {
        self.parent_target().map(|target| target.panel_path())
    }

    /// Shows Places through the same listing and commit paths as other destinations.
    pub fn replace_places(&mut self) -> Result<(), String> {
        self.navigate_target(PanelTarget::Places)
    }

    /// Returns the next visit's identity without changing history.
    pub fn history_target(&self, back: bool) -> Option<Location> {
        self.history_visit(back).map(|(location, _, _)| location)
    }

    fn travel(&mut self, back: bool) -> Result<bool, String> {
        let Some((location, selected, origin)) = self.history_visit(back) else {
            return Ok(false);
        };
        let target = PanelTarget::Location(location);
        let mut request = self.folder.listing_request(target.clone(), false, false);
        request.remote_origin = origin;
        let snapshot = self
            .service
            .list(&request, &AtomicBool::new(false))
            .map_err(|error| format!("{}: {error}", target.panel_path().display()))?;
        self.folder.commit_history_listing(snapshot, back, selected);
        Ok(true)
    }

    /// Drives Back through the production history commit.
    pub fn go_back(&mut self) -> Result<bool, String> {
        self.travel(true)
    }

    /// Drives Forward through the production history commit.
    pub fn go_forward(&mut self) -> Result<bool, String> {
        self.travel(false)
    }

    /// Drives entry activation, including explicit reveal of a search hit's folder.
    pub fn activate(&mut self) -> Result<Activation, String> {
        let Some(row) = self.current().cloned() else {
            return Ok(Activation::Nothing);
        };
        if let Some(target) = self.activation_target() {
            self.navigate_target(target)?;
            return Ok(Activation::Entered);
        }
        if self.results().is_some() {
            let parent = row
                .path
                .parent()
                .expect("search hit has a parent")
                .to_path_buf();
            let snapshot = self.snapshot(
                PanelTarget::Location(Location::local(parent).map_err(|error| error.to_string())?),
                false,
            )?;
            self.folder
                .commit_navigation_listing(snapshot, Some(row.location));
            return Ok(Activation::Entered);
        }
        Ok(Activation::OpenFile(row.location))
    }

    /// Supplies remote rows to an in-memory provider; production code builds parent rows.
    pub fn cache_remote(&mut self, path: PathBuf, entries: Vec<Entry>) {
        let location = RemoteLocation::parse(&path).unwrap().unwrap();
        let mut listings = self
            .fixtures
            .lock()
            .expect("fixture lock is never poisoned");
        listings.insert(location, entries);
        let names: HashSet<_> = listings
            .keys()
            .map(|location| location.remote().to_owned())
            .collect();
        drop(listings);
        let providers = names
            .into_iter()
            .map(|name| {
                Arc::new(FixtureProvider {
                    name,
                    listings: Arc::clone(&self.fixtures),
                }) as Arc<dyn Provider>
            })
            .collect();
        self.cache.clear();
        self.service =
            ListingService::with_cache(Arc::new(Registry::new(providers)), Arc::clone(&self.cache));
    }

    /// Mirrors panel content and listing-cache swapping in Commander.
    pub fn swap_with(&mut self, other: &mut Self) {
        self.folder.swap_with(&mut other.folder);
        std::mem::swap(&mut self.service, &mut other.service);
        std::mem::swap(&mut self.fixtures, &mut other.fixtures);
        std::mem::swap(&mut self.cache, &mut other.cache);
    }
}

/// A scratch folder with sub/, a.txt, b.txt and .hidden.
pub fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("b.txt"), "12345").unwrap();
    fs::write(dir.path().join("a.txt"), "1").unwrap();
    fs::write(dir.path().join(".hidden"), "").unwrap();
    dir
}

/// Visible row names for concise assertions.
pub fn names(folder: &Folder) -> Vec<&str> {
    folder
        .entries()
        .iter()
        .map(|row| row.name.as_str())
        .collect()
}

/// Loads an isolated model through the production service.
pub fn loaded(dir: &TempDir) -> TestFolder {
    let mut folder = TestFolder::new(dir.path().to_path_buf()).unwrap();
    assert_eq!(folder.load_or_ancestor(), None);
    folder
}

/// Refreshes a fixture's rows with its own local/archive/remote listing service.
pub fn reload(folder: &mut TestFolder) {
    folder.load().unwrap();
}
