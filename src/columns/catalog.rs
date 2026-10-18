//! Immutable built-in and provider column catalog with typed reader dispatch.

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::atomic::Ordering;

use crate::data_providers::{
    Applicability, ApplicabilityContext, DemandLimits, FieldDefinition, ProviderDefinition,
    RegistrationOrigin, ResultState, Subject, Value, validate_definitions,
};
use crate::location::Location;

use super::provider::{NativeColumnReader, ProviderColumnRequest};
use super::{ColumnId, ColumnKind};

/// How a column can be filtered from the values it displays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterBehavior {
    /// Case-insensitive substring matching is available.
    Contains,
    /// Exact category matching and substring matching are available.
    Categories,
    /// Filtering cannot acquire values for this column.
    Disabled(String),
}

/// Whether a saved column can currently produce cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability {
    /// A built-in implementation or registered provider is present.
    Available,
    /// The saved provider contribution is missing from this catalog.
    Unavailable(String),
}

/// Display and sort information for one canonical persisted column ID.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnDescriptor {
    /// Persisted identity.
    pub id: ColumnId,
    /// Table header title.
    pub title: String,
    /// Columns dialog label.
    pub label: String,
    /// Columns dialog group.
    pub group: String,
    /// Default table width.
    pub width: f32,
    /// Native implementation, when this is a built-in column.
    pub builtin: Option<ColumnKind>,
    /// Sorting is available only for built-in sortable columns.
    pub sort_key: Option<crate::entry::SortKey>,
    /// Whether the current catalog can produce values.
    pub availability: Availability,
    /// Supported filter editor for this column.
    pub filter_behavior: FilterBehavior,
}

struct ProviderColumn {
    descriptor: ColumnDescriptor,
    field: FieldDefinition,
    applicability: Applicability,
    demand: DemandLimits,
    reader: NativeColumnReader,
}

enum Entry {
    Builtin(ColumnDescriptor),
    Provider(ProviderColumn),
}

/// One validated provider bundle and its host-owned native column readers.
pub type NativeProviderRegistration = (
    RegistrationOrigin,
    ProviderDefinition,
    HashMap<crate::data_providers::ContributionId, NativeColumnReader>,
);

/// Immutable lookup for built-in and registered provider column definitions.
#[derive(Default)]
pub struct Catalog {
    entries: HashMap<ColumnId, Entry>,
    order: Vec<ColumnId>,
    field_ids: HashSet<crate::data_providers::FieldId>,
    provider_ids: HashSet<crate::data_providers::ProviderId>,
}

impl Catalog {
    /// Creates the production catalog with existing built-ins in current order.
    pub fn with_defaults() -> Self {
        Self::with_providers(Vec::new()).expect("an empty provider catalog is valid")
    }

    /// Creates a catalog with built-ins and atomically registered provider bundles.
    pub fn with_providers(registrations: Vec<NativeProviderRegistration>) -> io::Result<Self> {
        let mut catalog = Self::default();
        for kind in ColumnKind::ALL {
            let descriptor = builtin_descriptor(kind);
            debug_assert_eq!(descriptor.id.builtin(), Some(kind));
            catalog.order.push(descriptor.id.clone());
            catalog
                .entries
                .insert(descriptor.id.clone(), Entry::Builtin(descriptor));
        }
        catalog.register_all(registrations)?;
        Ok(catalog)
    }

    fn register_all(
        &mut self,
        registrations: impl IntoIterator<
            Item = (
                RegistrationOrigin,
                ProviderDefinition,
                HashMap<crate::data_providers::ContributionId, NativeColumnReader>,
            ),
        >,
    ) -> io::Result<()> {
        for (origin, definition, readers) in registrations {
            self.register_provider(origin, &definition, &readers)?;
        }
        Ok(())
    }

    /// Returns the registered descriptor for a canonical or legacy ID.
    pub fn descriptor(&self, id: &ColumnId) -> Option<&ColumnDescriptor> {
        self.entries.get(id).map(entry_descriptor)
    }

    /// Returns a registered descriptor or a removable unavailable placeholder.
    pub fn display_descriptor(&self, id: &ColumnId) -> ColumnDescriptor {
        self.descriptor(id)
            .cloned()
            .unwrap_or_else(|| ColumnDescriptor {
                id: id.clone(),
                title: id.as_str().into(),
                label: format!("{} — Provider unavailable", id.as_str()),
                group: "Unavailable".into(),
                width: 140.0,
                builtin: None,
                sort_key: None,
                availability: Availability::Unavailable("Provider unavailable".into()),
                filter_behavior: FilterBehavior::Disabled("Provider unavailable".into()),
            })
    }

    /// Returns dialog groups in catalog order, appending selected unavailable IDs.
    pub fn dialog_groups(&self, selected: &[ColumnId]) -> Vec<(String, Vec<ColumnDescriptor>)> {
        let mut groups: Vec<(String, Vec<ColumnDescriptor>)> = Vec::new();
        for id in &self.order {
            let descriptor = self.display_descriptor(id);
            push_group(&mut groups, descriptor);
        }
        for id in selected {
            if !self.entries.contains_key(id) {
                push_group(&mut groups, self.display_descriptor(id));
            }
        }
        groups
    }

    /// Registers all native column contributions from one provider atomically.
    pub fn register_provider(
        &mut self,
        origin: RegistrationOrigin,
        definition: &ProviderDefinition,
        readers: &HashMap<crate::data_providers::ContributionId, NativeColumnReader>,
    ) -> io::Result<()> {
        validate_definitions(&[(origin, definition.clone())])?;
        if self.provider_ids.contains(&definition.id) {
            return Err(invalid("provider ID is already registered"));
        }
        let mut pending = Vec::new();
        let mut registered_fields = HashSet::new();
        for column in &definition.columns {
            let id = ColumnId::new(column.column_id.clone())?;
            if id.builtin().is_some() {
                return Err(invalid(
                    "provider column ID collides with a built-in column",
                ));
            }
            if self.entries.contains_key(&id) || pending.iter().any(|(existing, _)| existing == &id)
            {
                return Err(invalid("column ID is already registered"));
            }
            let field = definition
                .fields
                .iter()
                .find(|field| field.id == column.field)
                .ok_or_else(|| invalid("column field is not declared"))?;
            if self.field_ids.contains(&field.id) {
                return Err(invalid("provider field ID is already registered"));
            }
            registered_fields.insert(field.id.clone());
            let reader = readers
                .get(&column.header.id)
                .cloned()
                .ok_or_else(|| invalid("provider column has no native reader"))?;
            let descriptor = ColumnDescriptor {
                id: id.clone(),
                title: column.header.label.clone(),
                label: column.header.label.clone(),
                group: definition.id.as_str().into(),
                width: 140.0,
                builtin: None,
                sort_key: None,
                availability: Availability::Available,
                filter_behavior: FilterBehavior::Contains,
            };
            pending.push((
                id,
                Entry::Provider(ProviderColumn {
                    descriptor,
                    field: field.clone(),
                    applicability: column.header.applicability.clone(),
                    demand: column.header.demand,
                    reader,
                }),
            ));
        }
        for field in registered_fields {
            self.field_ids.insert(field);
        }
        self.provider_ids.insert(definition.id.clone());
        for (id, entry) in pending {
            self.order.push(id.clone());
            self.entries.insert(id, entry);
        }
        Ok(())
    }

    /// Acquires and validates one registered provider cell; call only in a worker.
    pub fn read_provider_cell(
        &self,
        id: &ColumnId,
        request: &ProviderColumnRequest,
    ) -> ResultState<Value> {
        let Some(Entry::Provider(column)) = self.entries.get(id) else {
            return ResultState::Unavailable("provider column unavailable".into());
        };
        if request.demand != column.demand {
            return ResultState::Failed(
                "provider request bounds do not match its declaration".into(),
            );
        }
        if request.location.as_local_path().is_none() {
            return ResultState::Unavailable("provider columns require a local source".into());
        }
        let context = ApplicabilityContext {
            subject: Subject::Item(request.location.clone()),
            name: request.name.clone(),
            is_dir: request.is_dir,
            formats: Vec::new(),
            context_identity: None,
        };
        if !column.applicability.matches(&context) {
            return ResultState::Unavailable("provider does not apply to this row".into());
        }
        if request.cancel.load(Ordering::Relaxed) {
            return ResultState::Failed("cancelled".into());
        }
        match (column.reader)(request) {
            Ok(ResultState::Ready {
                value,
                completeness,
            }) => match column.field.validate(&value) {
                Ok(()) => ResultState::Ready {
                    value,
                    completeness,
                },
                Err(error) => ResultState::Failed(error.to_string()),
            },
            Ok(ResultState::Pending) => {
                ResultState::Failed("provider reader returned pending after completion".into())
            }
            Ok(state) => state,
            Err(error) => ResultState::Failed(error.to_string()),
        }
    }

    /// Returns the provider request bounds for a registered dynamic column.
    pub fn provider_demand(&self, id: &ColumnId) -> Option<DemandLimits> {
        match self.entries.get(id)? {
            Entry::Provider(column) => Some(column.demand),
            Entry::Builtin(_) => None,
        }
    }

    /// Reports applicability from captured listing facts only, without I/O.
    pub fn provider_applies(
        &self,
        id: &ColumnId,
        location: &Location,
        name: &str,
        is_dir: bool,
    ) -> bool {
        let Some(Entry::Provider(column)) = self.entries.get(id) else {
            return false;
        };
        column.applicability.matches(&ApplicabilityContext {
            subject: Subject::Item(location.clone()),
            name: name.into(),
            is_dir,
            formats: Vec::new(),
            context_identity: None,
        })
    }
}

fn entry_descriptor(entry: &Entry) -> &ColumnDescriptor {
    match entry {
        Entry::Builtin(descriptor) => descriptor,
        Entry::Provider(column) => &column.descriptor,
    }
}

fn builtin_descriptor(kind: ColumnKind) -> ColumnDescriptor {
    ColumnDescriptor {
        id: kind.into(),
        title: kind.title().into(),
        label: kind.label().into(),
        group: kind.group().to_string(),
        width: kind.width(),
        builtin: Some(kind),
        sort_key: kind.sort_key(),
        availability: Availability::Available,
        filter_behavior: match kind {
            ColumnKind::Ext
            | ColumnKind::GitStatus
            | ColumnKind::Owner
            | ColumnKind::Permissions
            | ColumnKind::Selinux
            | ColumnKind::Exif(
                crate::columns::ExifField::Camera | crate::columns::ExifField::Lens,
            ) => FilterBehavior::Categories,
            _ => FilterBehavior::Contains,
        },
    }
}

fn push_group(groups: &mut Vec<(String, Vec<ColumnDescriptor>)>, descriptor: ColumnDescriptor) {
    match groups.last_mut() {
        Some((group, columns)) if group == &descriptor.group => columns.push(descriptor),
        _ => groups.push((descriptor.group.clone(), vec![descriptor])),
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use super::{Availability, Catalog, ResultState};
    use crate::columns::ProviderColumnRequest;
    use crate::columns::{ColumnId, ColumnKind};
    use crate::data_providers::{
        Applicability, ColumnContribution, ContributionHeader, ContributionId, DemandLimits,
        FieldDefinition, FieldId, FieldKind, ProviderDefinition, ProviderId, RegistrationOrigin,
        Unit, Value,
    };
    use crate::location::Location;

    fn definition() -> ProviderDefinition {
        let provider = ProviderId::new("extension.sensor.reader").expect("provider ID");
        let field = FieldId::new("extension.sensor.reader.distance").expect("field ID");
        ProviderDefinition {
            id: provider,
            fields: vec![FieldDefinition {
                id: field.clone(),
                kind: FieldKind::Quantity(Unit::Metres),
            }],
            properties: Vec::new(),
            columns: vec![ColumnContribution {
                header: ContributionHeader {
                    id: ContributionId::new("extension.sensor.reader.distance-column")
                        .expect("contribution ID"),
                    label: "Distance".into(),
                    applicability: Applicability::Any,
                    demand: DemandLimits {
                        max_bytes: 64,
                        max_items: 8,
                    },
                },
                column_id: "extension.sensor.reader.distance".into(),
                field,
            }],
            viewers: Vec::new(),
        }
    }

    fn make_readers(value: Option<Value>) -> HashMap<ContributionId, super::NativeColumnReader> {
        let contribution = ContributionId::new("extension.sensor.reader.distance-column")
            .expect("contribution ID");
        HashMap::from([(
            contribution,
            Arc::new(move |_: &ProviderColumnRequest| {
                Ok(value
                    .clone()
                    .map_or(ResultState::Absent, |value| ResultState::Ready {
                        value,
                        completeness: crate::data_providers::Completeness::Complete,
                    }))
            }) as _,
        )])
    }

    fn request() -> ProviderColumnRequest {
        ProviderColumnRequest {
            location: Location::local(std::env::temp_dir().join("sensor.dat"))
                .expect("absolute path"),
            name: "sensor.dat".into(),
            is_dir: false,
            demand: DemandLimits {
                max_bytes: 64,
                max_items: 8,
            },
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn builtins_keep_current_descriptor_order_and_sort_rules() {
        let catalog = Catalog::with_defaults();
        for (index, kind) in ColumnKind::ALL.into_iter().enumerate() {
            let descriptor = catalog
                .descriptor(&ColumnId::from(kind))
                .expect("built-in descriptor");
            assert_eq!(catalog.order[index], ColumnId::from(kind));
            assert_eq!(descriptor.title, kind.title());
            assert_eq!(descriptor.label, kind.label());
            assert_eq!(descriptor.group, kind.group().to_string());
            assert_eq!(descriptor.width, kind.width());
            assert_eq!(descriptor.sort_key, kind.sort_key());
        }
        let first = catalog.descriptor(&ColumnKind::Name.into()).expect("Name");
        assert_eq!(first.title, "Name");
        assert_eq!(first.width, ColumnKind::Name.width());
        assert_eq!(first.sort_key, ColumnKind::Name.sort_key());
        let git = catalog
            .descriptor(&ColumnId::new("git").expect("legacy ID"))
            .expect("canonical Git descriptor");
        assert_eq!(git.id.as_str(), "git.status");
        assert_eq!(git.sort_key, None);
    }

    #[test]
    fn registration_is_atomic_and_cells_validate_declared_units() {
        let mut catalog = Catalog::with_defaults();
        let definition = definition();
        let native_readers =
            make_readers(Some(Value::quantity(12.0, Unit::Metres).expect("quantity")));
        catalog
            .register_provider(RegistrationOrigin::Extension, &definition, &native_readers)
            .expect("register sensor column");
        assert!(
            catalog
                .register_provider(RegistrationOrigin::Extension, &definition, &native_readers)
                .is_err()
        );
        let id = ColumnId::new("extension.sensor.reader.distance").expect("column ID");
        assert!(matches!(
            catalog.read_provider_cell(&id, &request()),
            crate::data_providers::ResultState::Ready { .. }
        ));

        let mut mismatch = Catalog::with_defaults();
        let bad_reader = make_readers(Some(
            Value::quantity(12.0, Unit::Seconds).expect("quantity"),
        ));
        mismatch
            .register_provider(RegistrationOrigin::Extension, &definition, &bad_reader)
            .expect("reader checked when requested");
        assert!(matches!(
            mismatch.read_provider_cell(&id, &request()),
            crate::data_providers::ResultState::Failed(_)
        ));

        let mut conflict = definition.clone();
        conflict.id = ProviderId::new("builtin.sensor").expect("builtin provider ID");
        let builtin_field = FieldId::new("builtin.sensor.distance").expect("builtin field ID");
        conflict.fields[0].id = builtin_field.clone();
        conflict.columns[0].header.id =
            ContributionId::new("builtin.sensor.distance-column").expect("builtin contribution ID");
        conflict.columns[0].field = builtin_field;
        conflict.columns[0].column_id = "size".into();
        let conflict_readers = make_readers(None);
        let mut atomic = Catalog::with_defaults();
        let collision = atomic
            .register_provider(RegistrationOrigin::Builtin, &conflict, &conflict_readers)
            .expect_err("provider cannot replace a built-in column");
        assert!(collision.to_string().contains("built-in"));
        assert!(atomic.descriptor(&id).is_none());
        let mut empty = Catalog::default();
        assert!(
            empty
                .register_provider(RegistrationOrigin::Builtin, &conflict, &conflict_readers)
                .is_err()
        );
    }

    #[test]
    fn provider_reader_preserves_completeness_and_rejects_completed_pending() {
        let definition = definition();
        let contribution = definition.columns[0].header.id.clone();
        let id = ColumnId::new(&definition.columns[0].column_id).expect("column ID");
        let partial_value = Value::quantity(5.0, Unit::Metres).expect("quantity");
        let readers = HashMap::from([(
            contribution.clone(),
            Arc::new(move |_: &ProviderColumnRequest| {
                Ok(ResultState::Ready {
                    value: partial_value.clone(),
                    completeness: crate::data_providers::Completeness::Truncated(
                        "sample limit".into(),
                    ),
                })
            }) as super::NativeColumnReader,
        )]);
        let mut partial_catalog = Catalog::with_defaults();
        partial_catalog
            .register_provider(RegistrationOrigin::Extension, &definition, &readers)
            .expect("register partial reader");
        assert!(matches!(
            partial_catalog.read_provider_cell(&id, &request()),
            ResultState::Ready {
                completeness: crate::data_providers::Completeness::Truncated(ref reason),
                ..
            } if reason == "sample limit"
        ));

        let pending_readers = HashMap::from([(
            contribution,
            Arc::new(|_: &ProviderColumnRequest| Ok(ResultState::Pending))
                as super::NativeColumnReader,
        )]);
        let mut pending_catalog = Catalog::with_defaults();
        pending_catalog
            .register_provider(RegistrationOrigin::Extension, &definition, &pending_readers)
            .expect("register pending reader");
        assert!(matches!(
            pending_catalog.read_provider_cell(&id, &request()),
            ResultState::Failed(reason) if reason.contains("pending after completion")
        ));
    }

    #[test]
    fn one_declared_field_can_back_multiple_columns_in_the_same_provider() {
        let mut definition = definition();
        let mut second = definition.columns[0].clone();
        second.header.id = ContributionId::new("extension.sensor.reader.distance-copy")
            .expect("second contribution");
        second.header.label = "Distance copy".into();
        second.column_id = "extension.sensor.reader.distance-copy".into();
        definition.columns.push(second);
        let first_id = definition.columns[0].header.id.clone();
        let second_id = definition.columns[1].header.id.clone();
        let reader: super::NativeColumnReader = std::sync::Arc::new(|_| Ok(ResultState::Absent));
        let readers = HashMap::from([(first_id, reader.clone()), (second_id, reader)]);
        let mut catalog = Catalog::with_defaults();
        catalog
            .register_provider(RegistrationOrigin::Extension, &definition, &readers)
            .expect("field reuse is allowed");
        assert!(
            catalog
                .descriptor(&ColumnId::new("extension.sensor.reader.distance-copy").unwrap())
                .is_some()
        );
    }

    #[test]
    fn unavailable_selected_ids_are_preserved_and_removable() {
        let catalog = Catalog::with_defaults();
        let id = ColumnId::new("extension.gone.field").expect("column ID");
        let descriptor = catalog.display_descriptor(&id);
        assert_eq!(
            descriptor.availability,
            Availability::Unavailable("Provider unavailable".into())
        );
        let groups = catalog.dialog_groups(std::slice::from_ref(&id));
        assert_eq!(groups.last().expect("unavailable group").0, "Unavailable");
    }

    #[test]
    fn selected_id_tracks_availability_across_catalog_rebuilds() {
        let selected = vec![ColumnId::new("extension.sensor.reader.distance").unwrap()];
        let definition = definition();
        let readers = make_readers(None);
        let mut available = Catalog::with_defaults();
        available
            .register_provider(RegistrationOrigin::Extension, &definition, &readers)
            .unwrap();
        assert_eq!(
            available.display_descriptor(&selected[0]).availability,
            Availability::Available
        );
        assert_eq!(
            available.dialog_groups(&selected).last().unwrap().1[0].id,
            selected[0]
        );

        let missing = Catalog::with_defaults();
        assert!(matches!(
            missing.display_descriptor(&selected[0]).availability,
            Availability::Unavailable(_)
        ));
        assert_eq!(
            missing.dialog_groups(&selected).last().unwrap().1[0].id,
            selected[0]
        );
        assert_eq!(selected[0].as_str(), "extension.sensor.reader.distance");

        let mut restored = Catalog::with_defaults();
        restored
            .register_provider(RegistrationOrigin::Extension, &definition, &readers)
            .unwrap();
        assert_eq!(
            restored.display_descriptor(&selected[0]).availability,
            Availability::Available
        );
        assert_eq!(
            restored.dialog_groups(&selected).last().unwrap().1[0].id,
            selected[0]
        );
    }
}
