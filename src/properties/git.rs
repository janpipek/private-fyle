//! Contextual Git snapshots projected into an independent native Properties group.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::data_providers::git::{self, Key, Service};
use crate::properties::{
    GroupId, GroupStatus, PropertyField, PropertyGroup, PropertyValue, Selection,
};

/// Properties adapter sharing acquisition with the existing Git column.
pub struct Provider {
    service: Service,
}

impl Provider {
    /// Retains the application-owned contextual Git service.
    pub fn new(service: Service) -> Self {
        Self { service }
    }

    /// Reads only original local selections inside the captured panel scope.
    pub fn read(
        &self,
        selection: &Selection,
        key: &Key,
        cancel: &Arc<AtomicBool>,
    ) -> io::Result<Vec<PropertyGroup>> {
        let Some(path) = selection.location.as_local_path() else {
            return Ok(Vec::new());
        };
        if path != selection.entry.path || !path.starts_with(&key.scope) {
            return Ok(Vec::new());
        }
        if cancel.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Git inspection cancelled",
            ));
        }
        match self
            .service
            .subscribe(key.clone())
            .and_then(|lease| lease.wait(cancel))
        {
            Ok(Some(snapshot)) => match git::project(&snapshot, path, selection.entry.is_dir) {
                Ok(facts) => Ok(vec![PropertyGroup {
                    id: self.group_id(),
                    status: GroupStatus::Available,
                    fields: vec![
                        PropertyField {
                            id: "repository".into(),
                            value: PropertyValue::Path(facts.root),
                        },
                        PropertyField {
                            id: "query_scope".into(),
                            value: PropertyValue::Path(facts.scope),
                        },
                        PropertyField {
                            id: "relative_path".into(),
                            value: PropertyValue::Path(facts.relative),
                        },
                        PropertyField {
                            id: "status".into(),
                            value: PropertyValue::Text(facts.status),
                        },
                    ],
                    groups: Vec::new(),
                    tables: Vec::new(),
                }]),
                Err(error) => Ok(vec![self.failed_group(error.to_string())]),
            },
            Ok(None) => Ok(Vec::new()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Err(error),
            Err(error) => Ok(vec![self.failed_group(error.to_string())]),
        }
    }

    fn group_id(&self) -> GroupId {
        GroupId(
            self.service.definition().properties[0]
                .header
                .id
                .as_str()
                .into(),
        )
    }

    fn failed_group(&self, reason: String) -> PropertyGroup {
        PropertyGroup {
            id: self.group_id(),
            status: GroupStatus::Failed(reason),
            fields: Vec::new(),
            groups: Vec::new(),
            tables: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Provider;
    use crate::data_providers::git::Service;
    use crate::entry::{Attrs, Entry};
    use crate::git_status::{Snapshot, Status};
    use crate::location::Location;
    use crate::properties::{GroupStatus, PropertyValue, Selection};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn selection(path: std::path::PathBuf) -> Selection {
        selection_kind(path, false)
    }

    fn selection_kind(path: std::path::PathBuf, is_dir: bool) -> Selection {
        Selection {
            location: Location::local(path.clone()).expect("absolute path"),
            entry: Entry {
                name: "file".into(),
                path,
                is_dir,
                is_parent: false,
                size: Some(1),
                modified: None,
                attrs: Attrs::default(),
                link: Default::default(),
            },
        }
    }

    fn snapshot(root: &std::path::Path, scope: &std::path::Path) -> Snapshot {
        Snapshot {
            root: root.to_path_buf(),
            scope: scope.to_path_buf(),
            canonical_scope: root.to_path_buf(),
            status: Status::parse(root, b" M dirty/child.txt\0!! ignored/\0?? new.txt\0"),
        }
    }

    #[test]
    fn contextual_failure_keeps_native_filesystem_facts() {
        let temp = tempfile::TempDir::new().expect("temporary directory");
        let path = temp.path().join("file");
        std::fs::write(&path, "data").expect("fixture");
        let service =
            Service::with_runner(|_, _| Err(std::io::Error::other("fixture Git failure")));
        let key = service.key_for_scope(temp.path());
        let mut registry = crate::properties::Registry::default();
        registry.register_git(service);
        let result = crate::properties::inspect_with_context(
            &selection(path),
            &Arc::new(AtomicBool::new(false)),
            &registry,
            Some(&key),
        )
        .expect("independent inspection");
        assert!(matches!(
            result.filesystem,
            crate::properties::Section::Available(_)
        ));
        assert!(
            matches!(&result.additional_groups[0].status, GroupStatus::Failed(reason) if reason == "fixture Git failure")
        );
    }

    #[test]
    fn rejects_nonlocal_mismatched_and_out_of_scope_before_acquisition() {
        let count = Arc::new(AtomicUsize::new(0));
        let launched = count.clone();
        let service = Service::with_runner(move |_, _| {
            launched.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        });
        let key = service.key_for_scope(std::path::Path::new("/repo"));
        let provider = Provider::new(service);
        let cancel = Arc::new(AtomicBool::new(false));
        let mut item = selection("/repo/file".into());
        item.location = Location::from_panel_path(std::path::Path::new("test:/file"), None)
            .expect("remote location");
        assert!(
            provider
                .read(&item, &key, &cancel)
                .expect("remote denied")
                .is_empty()
        );
        item.location = Location::local("/repo/other".into()).expect("local path");
        assert!(
            provider
                .read(&item, &key, &cancel)
                .expect("mismatch denied")
                .is_empty()
        );
        item = selection("/outside/file".into());
        assert!(
            provider
                .read(&item, &key, &cancel)
                .expect("outside denied")
                .is_empty()
        );
        item = selection("/repo/archive.zip/member".into());
        item.location = Location::archive(
            Location::local("/repo/archive.zip".into()).expect("archive source"),
            "member",
        )
        .expect("archive location");
        assert!(
            provider
                .read(&item, &key, &cancel)
                .expect("archive denied")
                .is_empty()
        );
        let cancelled = Arc::new(AtomicBool::new(true));
        let local = selection("/repo/file".into());
        assert_eq!(
            provider
                .read(&local, &key, &cancelled)
                .expect_err("cancelled before acquisition")
                .kind(),
            std::io::ErrorKind::Interrupted
        );
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn projects_typed_facts_for_directory_ignored_and_clean_entries() {
        let root = std::path::PathBuf::from("/repo");
        let service = Service::with_runner(|scope, _| Ok(Some(snapshot(scope, scope))));
        let key = service.key_for_scope(&root);
        let provider = Provider::new(service);
        let cancel = Arc::new(AtomicBool::new(false));

        let cases = [
            ("dirty", true, "Modified"),
            ("ignored", true, "Ignored"),
            ("clean.txt", false, "No changes reported"),
        ];
        for (relative, is_dir, expected) in cases {
            let selected = root.join(relative);
            let groups = provider
                .read(&selection_kind(selected, is_dir), &key, &cancel)
                .expect("Git group projection");
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].status, GroupStatus::Available);
            assert_eq!(groups[0].fields[0].id, "repository");
            assert_eq!(groups[0].fields[0].value, PropertyValue::Path(root.clone()));
            assert_eq!(groups[0].fields[1].id, "query_scope");
            assert_eq!(groups[0].fields[1].value, PropertyValue::Path(root.clone()));
            assert_eq!(groups[0].fields[2].id, "relative_path");
            assert_eq!(
                groups[0].fields[2].value,
                PropertyValue::Path(relative.into())
            );
            assert_eq!(groups[0].fields[3].id, "status");
            assert_eq!(
                groups[0].fields[3].value,
                PropertyValue::Text(expected.into())
            );
        }
    }

    #[test]
    fn properties_reuses_the_column_leases_cached_snapshot() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner_calls = calls.clone();
        let service = Service::with_runner(move |scope, _| {
            runner_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Some(snapshot(scope, scope)))
        });
        let root = std::path::PathBuf::from("/repo");
        let key = service.key_for_scope(&root);
        let cancel = AtomicBool::new(false);
        let column_snapshot = service
            .subscribe(key.clone())
            .expect("column lease")
            .wait(&cancel)
            .expect("column result")
            .expect("repository snapshot");

        let groups = Provider::new(service.clone())
            .read(
                &selection(root.join("new.txt")),
                &key,
                &Arc::new(AtomicBool::new(false)),
            )
            .expect("Properties projection");
        assert_eq!(
            groups[0].fields[3].value,
            PropertyValue::Text("Untracked".into())
        );

        let property_snapshot = service
            .subscribe(key)
            .expect("cached property lease")
            .wait(&cancel)
            .expect("cached result")
            .expect("cached snapshot");
        assert!(Arc::ptr_eq(&column_snapshot, &property_snapshot));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[cfg(unix)]
    #[test]
    fn lexical_symlink_selection_maps_through_canonical_snapshot_scope() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::TempDir::new().expect("temporary directory");
        let root = temp.path().join("repo");
        let actual_scope = root.join("nested");
        let lexical_scope = temp.path().join("alias");
        std::fs::create_dir_all(&actual_scope).expect("actual scope");
        symlink(&actual_scope, &lexical_scope).expect("scope symlink");
        let selected = lexical_scope.join("new.txt");
        let service = Service::with_runner({
            let root = root.clone();
            let actual_scope = actual_scope.clone();
            let lexical_scope = lexical_scope.clone();
            move |_, _| {
                Ok(Some(Snapshot {
                    root: root.clone(),
                    scope: lexical_scope.clone(),
                    canonical_scope: actual_scope.clone(),
                    status: Status::parse(&root, b"?? nested/new.txt\0"),
                }))
            }
        });
        let key = service.key_for_scope(&lexical_scope);
        let groups = Provider::new(service)
            .read(
                &selection(selected),
                &key,
                &Arc::new(AtomicBool::new(false)),
            )
            .expect("symlink spelling projects");
        assert_eq!(groups[0].fields[0].value, PropertyValue::Path(root));
        assert_eq!(
            groups[0].fields[1].value,
            PropertyValue::Path(lexical_scope)
        );
        assert_eq!(
            groups[0].fields[2].value,
            PropertyValue::Path("nested/new.txt".into())
        );
        assert_eq!(
            groups[0].fields[3].value,
            PropertyValue::Text("Untracked".into())
        );
    }

    #[test]
    fn projection_failure_is_a_failed_git_group_not_an_inspection_error() {
        let temp = tempfile::TempDir::new().expect("temporary directory");
        let path = temp.path().join("file");
        std::fs::write(&path, "fixture").expect("local file");
        let service = Service::with_runner(|scope, _| {
            let narrower = scope.join("inside");
            Ok(Some(Snapshot {
                root: scope.to_path_buf(),
                scope: narrower.clone(),
                canonical_scope: narrower,
                status: Status::default(),
            }))
        });
        let key = service.key_for_scope(temp.path());
        let mut registry = crate::properties::Registry::default();
        registry.register_git(service);
        let result = crate::properties::inspect_with_context(
            &selection(path),
            &Arc::new(AtomicBool::new(false)),
            &registry,
            Some(&key),
        )
        .expect("independent section failures do not fail inspection");
        assert!(matches!(
            result.filesystem,
            crate::properties::Section::Available(_)
        ));
        assert!(matches!(
            &result.additional_groups[0].status,
            GroupStatus::Failed(reason) if reason == "selected path is outside Git scope"
        ));
    }
}
