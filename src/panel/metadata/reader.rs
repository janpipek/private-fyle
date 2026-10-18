//! Blocking reads of optional local metadata for panel rows.

use crate::data_providers::media as media_provider;
use crate::formats::media;
use crate::fs_ops;
use std::sync::atomic::Ordering;

use super::exif;
use super::model::{Field, Request, Values};

/// Reads only requested fields; missing optional metadata keeps its existing empty value.
pub fn read_metadata(request: &Request, catalog: &crate::columns::Catalog) -> Values {
    let path = request.path();
    let mut values = Values::default();
    if request.fields.contains(&Field::Recognition) && !request.cancel.load(Ordering::Relaxed) {
        let head = crate::recognition::read_head(path).ok();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        values.recognition = crate::recognition::detect(
            &name,
            false,
            head.as_deref(),
            None,
            &crate::recognition::NativeDetector,
        )
        .into_iter()
        .next();
    }
    if request.fields.contains(&Field::Duration) {
        let duration = media_provider::duration_column()
            .read(path, || request.cancel.load(Ordering::Relaxed))
            .ok()
            .and_then(|prepared| match prepared.duration {
                crate::data_providers::ResultState::Ready { value, .. } => {
                    media::format_duration(value.value())
                }
                _ => None,
            })
            .unwrap_or_default();
        values.duration = Some(duration);
    }
    if request.fields.contains(&Field::Xattrs) {
        values.xattrs = Some(crate::xattr::names(path).join(", "));
    }
    if request.fields.contains(&Field::Selinux) {
        values.selinux = Some(crate::xattr::selinux_context(path).unwrap_or_default());
    }
    if request.fields.contains(&Field::Exif) {
        values.exif = Some(exif::exif_fields(path));
    }
    if request.fields.contains(&Field::Owner) {
        values.owner = Some(fs_ops::owner_name(request.uid));
    }
    if request.fields.contains(&Field::ResolvedLink) {
        values.resolved_link = Some(request.link.resolved(path));
    }
    for field in &request.fields {
        if let Field::Provider(id) = field {
            let result = catalog.read_provider_cell(
                id,
                &crate::columns::ProviderColumnRequest {
                    location: crate::location::Location::local(path.to_path_buf())
                        .expect("request path is a local absolute path"),
                    name: request
                        .row
                        .as_ref()
                        .map_or_else(String::new, |row| row.name.clone()),
                    is_dir: request.row.as_ref().is_some_and(|row| row.is_dir),
                    demand: catalog.provider_demand(id).unwrap_or(
                        crate::data_providers::DemandLimits {
                            max_bytes: 0,
                            max_items: 0,
                        },
                    ),
                    cancel: request.cancel.clone(),
                },
            );
            values.providers.insert(id.clone(), result);
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::read_metadata;
    use crate::columns::Catalog;
    use crate::columns::ColumnId;
    use crate::columns::provider::NativeColumnReader;
    use crate::data_providers::{
        ColumnContribution, ContributionHeader, ContributionId, DemandLimits, FieldDefinition,
        FieldId, FieldKind, ProviderDefinition, ProviderId, RegistrationOrigin, ResultState, Unit,
        Value,
    };
    use crate::links::LinkState;
    use crate::location::Location;
    use crate::panel::metadata::model::{CapturedRow, Field, MetadataQueue};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use tempfile::TempDir;

    #[test]
    fn recognition_flows_through_queue_and_obsolete_results_are_rejected() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("unknown");
        std::fs::write(&path, b"SQLite format 3\0").unwrap();
        let location = Location::local(path.clone()).unwrap();
        let mut queue = MetadataQueue::default();
        queue.request(
            &location,
            1,
            &[Field::Recognition],
            None,
            LinkState::NotLink,
        );
        let request = queue.take_next().unwrap();
        let values = read_metadata(&request, &Catalog::with_defaults());
        assert_eq!(
            values.recognition.as_ref().unwrap().mime,
            "application/vnd.sqlite3"
        );
        queue.sync_generation(2);
        assert!(!queue.complete(request, values));
        assert!(queue.get(&path, 2).is_none());
        queue.request(
            &location,
            2,
            &[Field::Recognition],
            None,
            LinkState::NotLink,
        );
        let request = queue.take_next().unwrap();
        let values = read_metadata(&request, &Catalog::with_defaults());
        assert!(queue.complete(request, values));
        assert_eq!(
            queue.get(&path, 2).unwrap().recognition().unwrap().mime,
            "application/vnd.sqlite3"
        );
    }

    #[test]
    fn reader_leaves_unrequested_fields_absent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("plain.txt");
        std::fs::write(&path, "plain text").unwrap();
        let mut queue = MetadataQueue::default();
        queue.request(
            &Location::local(path).unwrap(),
            1,
            &[Field::Exif],
            None,
            LinkState::NotLink,
        );
        let values = read_metadata(&queue.take_next().unwrap(), &Catalog::with_defaults());
        assert!(values.exif.unwrap().is_empty());
        assert!(values.xattrs.is_none());
        assert!(values.selinux.is_none());
        assert!(values.owner.is_none());
        assert!(values.resolved_link.is_none());
    }

    #[test]
    fn missing_optional_metadata_returns_completed_empty_values() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("missing.jpg");
        let mut queue = MetadataQueue::default();
        queue.request(
            &Location::local(path).unwrap(),
            1,
            &[
                Field::Xattrs,
                Field::Selinux,
                Field::Exif,
                Field::ResolvedLink,
            ],
            None,
            LinkState::Link {
                target: "missing".into(),
            },
        );
        let values = read_metadata(&queue.take_next().unwrap(), &Catalog::with_defaults());
        assert_eq!(values.xattrs.as_deref(), Some(""));
        assert_eq!(values.selinux.as_deref(), Some(""));
        assert!(values.exif.unwrap().is_empty());
        assert_eq!(values.resolved_link, Some(None));
        assert!(values.owner.is_none());
    }

    #[test]
    fn provider_value_flows_through_existing_queue_and_typed_cache() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("route.dat");
        std::fs::write(&path, "fixture").unwrap();
        let provider = ProviderId::new("extension.sensor.reader").unwrap();
        let field = FieldId::new("extension.sensor.reader.distance").unwrap();
        let contribution = ContributionId::new("extension.sensor.reader.distance-cell").unwrap();
        let demand = DemandLimits {
            max_bytes: 16,
            max_items: 2,
        };
        let definition = ProviderDefinition {
            id: provider,
            fields: vec![FieldDefinition {
                id: field.clone(),
                kind: FieldKind::Quantity(Unit::Metres),
            }],
            properties: Vec::new(),
            columns: vec![ColumnContribution {
                header: ContributionHeader {
                    id: contribution.clone(),
                    label: "Distance".into(),
                    applicability: crate::data_providers::Applicability::Any,
                    demand,
                },
                column_id: "extension.sensor.reader.distance".into(),
                field,
            }],
            viewers: Vec::new(),
        };
        let expected_path = path.clone();
        let reader: NativeColumnReader = Arc::new(move |request| {
            if request.location.as_local_path() != Some(expected_path.as_path())
                || request.name != "route.dat"
                || request.is_dir
                || request.demand != demand
                || request.cancel.load(Ordering::Relaxed)
            {
                return Err(std::io::Error::other("captured request facts changed"));
            }
            Ok(ResultState::Ready {
                value: Value::quantity(42.5, Unit::Metres).unwrap(),
                completeness: crate::data_providers::Completeness::Complete,
            })
        });
        let mut catalog = Catalog::with_defaults();
        catalog
            .register_provider(
                RegistrationOrigin::Extension,
                &definition,
                &HashMap::from([(contribution, reader)]),
            )
            .unwrap();

        let id = ColumnId::new("extension.sensor.reader.distance").unwrap();
        let location = Location::local(path.clone()).unwrap();
        let mut queue = MetadataQueue::default();
        queue.request_row(
            &location,
            3,
            &[Field::Provider(id.clone())],
            None,
            LinkState::NotLink,
            Some(CapturedRow {
                name: "route.dat".into(),
                is_dir: false,
            }),
        );
        let request = queue.take_next().unwrap();
        let values = read_metadata(&request, &catalog);
        assert!(queue.complete(request, values));
        assert!(matches!(
            queue.get(&path, 3).and_then(|values| values.provider_cell(&id)),
            Some(ResultState::Ready { value: Value::Quantity(quantity), .. }) if quantity.value() == 42.5 && quantity.unit() == Unit::Metres
        ));
        queue.request(
            &location,
            3,
            &[Field::Provider(id)],
            None,
            LinkState::NotLink,
        );
        assert!(queue.take_next().is_none());
    }

    #[test]
    fn absent_and_failed_provider_cells_are_cached_without_retry() {
        use crate::data_providers::{
            Applicability, ColumnContribution, ContributionHeader, ContributionId, DemandLimits,
            FieldDefinition, FieldId, FieldKind, ProviderDefinition, ProviderId,
            RegistrationOrigin, ResultState, Unit, Value,
        };
        use std::sync::atomic::AtomicUsize;

        #[derive(Clone, Copy)]
        enum Outcome {
            Absent,
            Error,
            WrongUnit,
        }

        for outcome in [Outcome::Absent, Outcome::Error, Outcome::WrongUnit] {
            let temp = TempDir::new().unwrap();
            let path = temp.path().join("row.dat");
            let provider = ProviderId::new("extension.sensor.reader").unwrap();
            let field = FieldId::new("extension.sensor.reader.distance").unwrap();
            let contribution =
                ContributionId::new("extension.sensor.reader.distance-cell").unwrap();
            let id = ColumnId::new("extension.sensor.reader.distance").unwrap();
            let demand = DemandLimits {
                max_bytes: 8,
                max_items: 1,
            };
            let definition = ProviderDefinition {
                id: provider,
                fields: vec![FieldDefinition {
                    id: field.clone(),
                    kind: FieldKind::Quantity(Unit::Metres),
                }],
                properties: Vec::new(),
                columns: vec![ColumnContribution {
                    header: ContributionHeader {
                        id: contribution.clone(),
                        label: "Distance".into(),
                        applicability: Applicability::Any,
                        demand,
                    },
                    column_id: id.as_str().into(),
                    field,
                }],
                viewers: Vec::new(),
            };
            let calls = Arc::new(AtomicUsize::new(0));
            let reader_calls = calls.clone();
            let reader: NativeColumnReader = Arc::new(move |_| {
                reader_calls.fetch_add(1, Ordering::Relaxed);
                match outcome {
                    Outcome::Absent => Ok(ResultState::Absent),
                    Outcome::Error => Err(std::io::Error::other("fixture failure")),
                    Outcome::WrongUnit => Ok(ResultState::Ready {
                        value: Value::quantity(7.0, Unit::Seconds).unwrap(),
                        completeness: crate::data_providers::Completeness::Complete,
                    }),
                }
            });
            let catalog = Catalog::with_providers(vec![(
                RegistrationOrigin::Extension,
                definition,
                HashMap::from([(contribution, reader)]),
            )])
            .unwrap();

            let location = Location::local(path.clone()).unwrap();
            let mut queue = MetadataQueue::default();
            queue.request(
                &location,
                1,
                &[Field::Provider(id.clone())],
                None,
                LinkState::NotLink,
            );
            let request = queue.take_next().unwrap();
            let values = read_metadata(&request, &catalog);
            assert!(queue.complete(request, values));
            let cell = queue.get(&path, 1).unwrap().provider_cell(&id).unwrap();
            match outcome {
                Outcome::Absent => assert_eq!(cell, &ResultState::Absent),
                Outcome::Error | Outcome::WrongUnit => {
                    assert!(matches!(cell, ResultState::Failed(_)))
                }
            }
            queue.request(
                &location,
                1,
                &[Field::Provider(id)],
                None,
                LinkState::NotLink,
            );
            assert!(queue.take_next().is_none());
            assert_eq!(calls.load(Ordering::Relaxed), 1);
        }
    }
}
