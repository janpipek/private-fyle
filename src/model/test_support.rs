//! Shared fixtures for the folder model tests.

use std::fs;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tempfile::TempDir;

use super::Folder;
use crate::listing::ListingService;
use crate::remote::Registry;

/// A scratch folder with `sub/`, `a.txt`, `b.txt` and `.hidden`.
pub fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("b.txt"), "12345").unwrap();
    fs::write(dir.path().join("a.txt"), "1").unwrap();
    fs::write(dir.path().join(".hidden"), "").unwrap();
    dir
}

pub fn names(folder: &Folder) -> Vec<&str> {
    folder.entries().iter().map(|e| e.name.as_str()).collect()
}

pub fn loaded(dir: &TempDir) -> Folder {
    let mut folder = Folder::new(dir.path().to_path_buf()).unwrap();
    assert_eq!(folder.load_or_ancestor(), None);
    folder
}

pub fn reload(folder: &mut Folder) {
    let service = ListingService::new(Arc::new(Registry::default()));
    let request = folder.listing_request(folder.target(), false, false);
    let snapshot = service.list(&request, &AtomicBool::new(false)).unwrap();
    let selected = folder.current().map(|row| row.location.clone());
    folder.apply_listing(snapshot, selected);
}
