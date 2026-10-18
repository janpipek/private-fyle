//! Bounded Parquet metadata mapped into a Properties group.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::data_providers::parquet as parquet_provider;
use crate::formats::parquet::check_cancel;
use crate::properties::{
    GroupId, GroupStatus, Probe, PropertyField, PropertyGroup, PropertyProvider, PropertyTable,
    PropertyValue,
};

const MAX_COLUMNS: usize = 256;
const MAX_ROW_GROUPS: usize = 128;
const MAX_TEXT_CHARS: usize = 1024;

/// Parquet metadata provider for the Properties registry.
pub(super) struct Provider;

impl PropertyProvider for Provider {
    fn id(&self) -> &'static str {
        "parquet"
    }

    fn matches(&self, probe: &Probe) -> bool {
        probe.is_file && parquet_provider::properties().matches_path(&probe.path)
    }

    fn read(&self, probe: &Probe, cancel: &Arc<AtomicBool>) -> io::Result<Vec<PropertyGroup>> {
        read(&probe.path, cancel).map(|group| vec![group])
    }
}

/// Builds one semantic Properties group from a local regular Parquet file.
pub fn read(path: &Path, cancel: &AtomicBool) -> io::Result<PropertyGroup> {
    let metadata = parquet_provider::properties().read_metadata(path, cancel)?;
    let schema = metadata.file_metadata().schema_descr();
    let columns = schema.columns();
    let mut truncations = Vec::new();
    if columns.len() > MAX_COLUMNS {
        truncations.push(format!("schema limited to {MAX_COLUMNS} leaf columns"));
    }
    let rows = metadata.row_groups();
    if rows.len() > MAX_ROW_GROUPS {
        truncations.push(format!("row groups limited to {MAX_ROW_GROUPS}"));
    }

    let mut schema_rows = Vec::new();
    for column in columns.iter().take(MAX_COLUMNS) {
        check_cancel(Some(cancel))?;
        schema_rows.push(vec![
            text(column.path().string(), &mut truncations),
            text(format!("{:?}", column.physical_type()), &mut truncations),
            text(
                column
                    .logical_type_ref()
                    .map_or("—".into(), |v| format!("{v:?}")),
                &mut truncations,
            ),
            PropertyValue::Signed(i64::from(column.max_def_level())),
            PropertyValue::Signed(i64::from(column.max_rep_level())),
        ]);
    }

    let mut row_group_rows = Vec::new();
    for (index, group) in rows.iter().take(MAX_ROW_GROUPS).enumerate() {
        check_cancel(Some(cancel))?;
        let mut compressed = 0u64;
        let mut uncompressed = 0u64;
        for column in group.columns() {
            check_cancel(Some(cancel))?;
            compressed = compressed
                .checked_add(u64::try_from(column.compressed_size()).map_err(invalid)?)
                .ok_or_else(|| invalid("Parquet compressed byte count overflow"))?;
            uncompressed = uncompressed
                .checked_add(u64::try_from(column.uncompressed_size()).map_err(invalid)?)
                .ok_or_else(|| invalid("Parquet uncompressed byte count overflow"))?;
        }
        row_group_rows.push(vec![
            PropertyValue::Unsigned(index as u64 + 1),
            PropertyValue::Unsigned(u64::try_from(group.num_rows()).map_err(invalid)?),
            PropertyValue::Unsigned(compressed),
            PropertyValue::Unsigned(uncompressed),
        ]);
    }
    check_cancel(Some(cancel))?;
    let mut fields = vec![PropertyField {
        id: "row_count".into(),
        value: PropertyValue::Unsigned(
            u64::try_from(metadata.file_metadata().num_rows()).map_err(invalid)?,
        ),
    }];
    fields.extend([
        PropertyField {
            id: "schema_columns".into(),
            value: PropertyValue::Unsigned(columns.len() as u64),
        },
        PropertyField {
            id: "row_group_count".into(),
            value: PropertyValue::Unsigned(rows.len() as u64),
        },
    ]);
    if let Some(created_by) = metadata.file_metadata().created_by() {
        fields.push(PropertyField {
            id: "created_by".into(),
            value: text(created_by.to_owned(), &mut truncations),
        });
    }
    fields.push(PropertyField {
        id: "version".into(),
        value: PropertyValue::Signed(i64::from(metadata.file_metadata().version())),
    });
    Ok(PropertyGroup {
        id: GroupId("parquet".into()),
        status: if truncations.is_empty() {
            GroupStatus::Available
        } else {
            GroupStatus::Truncated {
                reason: format!("Truncated: {}", truncations.join("; ")),
            }
        },
        fields,
        groups: Vec::new(),
        tables: vec![
            PropertyTable {
                id: "schema".into(),
                columns: vec![
                    "path".into(),
                    "physical_type".into(),
                    "logical_type".into(),
                    "definition_level".into(),
                    "repetition_level".into(),
                ],
                rows: schema_rows,
            },
            PropertyTable {
                id: "row_groups".into(),
                columns: vec![
                    "index".into(),
                    "rows".into(),
                    "compressed_bytes".into(),
                    "uncompressed_bytes".into(),
                ],
                rows: row_group_rows,
            },
        ],
    })
}

fn text(value: String, truncations: &mut Vec<String>) -> PropertyValue {
    if value.chars().count() <= MAX_TEXT_CHARS {
        return PropertyValue::Text(value);
    }
    let cap = format!("text limited to {MAX_TEXT_CHARS} characters");
    if !truncations.contains(&cap) {
        truncations.push(cap);
    }
    PropertyValue::Text(
        value
            .chars()
            .take(MAX_TEXT_CHARS - 1)
            .chain(['…'])
            .collect(),
    )
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::io;
    use std::path::Path;
    use std::sync::Arc;

    use parquet::file::writer::SerializedFileWriter;
    use parquet::schema::parser::parse_message_type;
    use tempfile::TempDir;

    use super::{MAX_COLUMNS, read};
    use crate::formats::parquet::{MAX_METADATA_BYTES, read_metadata};
    use crate::properties::{GroupStatus, PropertyValue};

    fn empty_file(path: &Path, schema: &str) {
        let schema = Arc::new(parse_message_type(schema).expect("valid test schema"));
        let writer = SerializedFileWriter::new(
            File::create(path).expect("create fixture"),
            schema,
            Default::default(),
        )
        .expect("create writer");
        writer.close().expect("close writer");
    }

    #[test]
    fn reads_nested_repeated_schema_and_group_facts_without_preview_decoding() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("nested.parquet");
        empty_file(
            &path,
            "message sample { REQUIRED GROUP record { OPTIONAL INT32 value; } REPEATED INT64 values; }",
        );
        let group = read(&path, &Default::default()).expect("metadata-only read");
        assert!(matches!(group.status, GroupStatus::Available));
        let schema = group
            .tables
            .iter()
            .find(|table| table.id == "schema")
            .expect("schema table");
        assert_eq!(schema.rows.len(), 2);
        assert_eq!(
            schema.rows[0][0],
            PropertyValue::Text("record.value".into())
        );
        assert_eq!(schema.rows[0][3], PropertyValue::Signed(1));
        assert_eq!(schema.rows[1][4], PropertyValue::Signed(1));
        assert!(
            group
                .tables
                .iter()
                .find(|table| table.id == "row_groups")
                .expect("groups table")
                .rows
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn reads_parquet_through_valid_symlink() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new().expect("temp dir");
        let target = dir.path().join("target.parquet");
        let link = dir.path().join("link.parquet");
        empty_file(&target, "message sample { REQUIRED INT32 value; }");
        symlink(&target, &link).expect("symlink");
        assert!(
            read(&link, &Default::default())
                .expect("read symlink target")
                .is_visible()
        );
    }

    #[test]
    fn validates_corrupt_and_oversized_footer_and_honors_precancellation() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("bad.parquet");
        for bytes in [
            b"not parquet".to_vec(),
            [
                b"PAR1".as_slice(),
                &((MAX_METADATA_BYTES + 1) as u32).to_le_bytes(),
                b"PAR1",
            ]
            .concat(),
        ] {
            fs::write(&path, bytes).expect("write bad fixture");
            assert!(read(&path, &Default::default()).is_err());
        }
        let mut file = File::open(&path).expect("open fixture");
        let cancelled = std::sync::atomic::AtomicBool::new(true);
        assert_eq!(
            read_metadata(&mut file, Some(&cancelled))
                .expect_err("cancelled")
                .kind(),
            io::ErrorKind::Interrupted
        );
    }

    #[test]
    fn caps_schema_columns_and_keeps_truncation_textual() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("wide.parquet");
        let fields = (0..MAX_COLUMNS + 1)
            .map(|i| format!("REQUIRED INT32 c{i};"))
            .collect::<String>();
        empty_file(&path, &format!("message sample {{ {fields} }}"));
        let group = read(&path, &Default::default()).expect("read wide schema");
        let schema = group
            .tables
            .iter()
            .find(|table| table.id == "schema")
            .expect("schema table");
        assert_eq!(schema.rows.len(), MAX_COLUMNS);
        assert!(
            matches!(&group.status, GroupStatus::Truncated { reason } if reason.contains("256 leaf columns"))
        );
    }

    #[test]
    fn reads_row_group_counts_and_checked_chunk_sizes() {
        use parquet::data_type::Int32Type;
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("rows.parquet");
        let schema = Arc::new(
            parse_message_type("message sample { REQUIRED INT32 value; }").expect("schema"),
        );
        let mut writer = SerializedFileWriter::new(
            File::create(&path).expect("file"),
            schema,
            Default::default(),
        )
        .expect("writer");
        for values in [&[1i32, 2][..], &[3i32][..]] {
            let mut row_group = writer.next_row_group().expect("row group");
            let mut column = row_group
                .next_column()
                .expect("column")
                .expect("column exists");
            column
                .typed::<Int32Type>()
                .write_batch(values, None, None)
                .expect("write values");
            column.close().expect("close column");
            row_group.close().expect("close row group");
        }
        writer.close().expect("close writer");
        let group = read(&path, &Default::default()).expect("metadata read");
        assert_eq!(
            group
                .fields
                .iter()
                .find(|field| field.id == "row_count")
                .expect("count")
                .value,
            PropertyValue::Unsigned(3)
        );
        let table = group
            .tables
            .iter()
            .find(|table| table.id == "row_groups")
            .expect("table");
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0][0], PropertyValue::Unsigned(1));
        assert_eq!(table.rows[0][1], PropertyValue::Unsigned(2));
        let compressed = match table.rows[0][2] {
            PropertyValue::Unsigned(size) => size,
            _ => panic!("compressed size is unsigned"),
        };
        let uncompressed = match table.rows[0][3] {
            PropertyValue::Unsigned(size) => size,
            _ => panic!("uncompressed size is unsigned"),
        };
        assert!(compressed > 0);
        assert!(uncompressed >= compressed);
    }
}
