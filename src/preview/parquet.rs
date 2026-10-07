//! Bounded Parquet row samples, rendered by the shared table preview.

use std::fmt::{self, Write as _};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::Field;
use parquet::schema::types::Type;

use super::{Content, Previewer, Probe};

const MAX_ROWS: usize = 500;
const MAX_COLS: usize = 40;
const MAX_CELL_CHARS: usize = 1024;
const MAX_METADATA_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SAMPLE_BYTES: u64 = 64 * 1024 * 1024;

/// Native Parquet content preview, without an external converter or Arrow.
pub struct ParquetPreviewer;

impl Previewer for ParquetPreviewer {
    fn name(&self) -> &'static str {
        "Parquet"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && (probe.ext == "parquet" || probe.head.starts_with(b"PAR1"))
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        let mut file = File::open(&probe.path)?;
        check_footer(&mut file)?;
        let reader = SerializedFileReader::new(file).map_err(invalid_data)?;
        let metadata = reader.metadata();
        let schema = metadata.file_metadata().schema_descr();
        let root = schema.root_schema();
        let fields = root.get_fields().iter().take(MAX_COLS).cloned().collect();
        let projection = Type::group_type_builder(root.name())
            .with_fields(fields)
            .build()
            .map_err(invalid_data)?;
        // The record API cannot reliably decode repeated fields. Refuse these
        // explicitly rather than presenting potentially incorrect rows.
        if schema.columns().iter().enumerate().any(|(i, column)| {
            schema.get_column_root_idx(i) < MAX_COLS && column.max_rep_level() > 0
        }) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Parquet preview does not yet support list, map or repeated columns",
            ));
        }
        let mut truncated = root.get_fields().len() > MAX_COLS;
        let header = projection
            .get_fields()
            .iter()
            .map(|field| cell(field.name(), &mut truncated))
            .collect();
        let mut rows = Vec::new();
        let mut remaining_bytes = MAX_SAMPLE_BYTES;
        for (i, group) in metadata.row_groups().iter().enumerate() {
            if rows.len() == MAX_ROWS {
                break;
            }
            if group.num_rows() == 0 {
                continue;
            }
            // A row limit alone does not bound column chunks/dictionaries.
            // Charge the selected chunks before decoding a row group; groups
            // beyond the sample are never opened.
            let bytes = selected_chunk_bytes(group.columns().iter().enumerate().filter_map(
                |(column_index, column)| {
                    (schema.get_column_root_idx(column_index) < MAX_COLS)
                        .then_some((column.compressed_size(), column.uncompressed_size()))
                },
            ))?;
            if bytes > remaining_bytes {
                if rows.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Parquet sample exceeds the 64 MiB column-chunk decoding limit",
                    ));
                }
                truncated = true;
                break;
            }
            remaining_bytes -= bytes;
            let group_reader = reader.get_row_group(i).map_err(invalid_data)?;
            for row in group_reader
                .get_row_iter(Some(projection.clone()))
                .map_err(invalid_data)?
                .take(MAX_ROWS - rows.len())
            {
                let row = row.map_err(invalid_data)?;
                rows.push(
                    row.get_column_iter()
                        .map(|(_, field)| field_cell(field, &mut truncated))
                        .collect(),
                );
            }
        }
        let total = u64::try_from(metadata.file_metadata().num_rows()).map_err(invalid_data)?;
        truncated |= total > rows.len() as u64;
        Ok(Content::Table {
            header,
            rows,
            truncated,
        })
    }
}

fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn selected_chunk_bytes(sizes: impl IntoIterator<Item = (i64, i64)>) -> io::Result<u64> {
    sizes
        .into_iter()
        .try_fold(0u64, |sum, (compressed, uncompressed)| {
            let size = u64::try_from(compressed.max(uncompressed)).map_err(invalid_data)?;
            sum.checked_add(size).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Parquet chunk sizes overflow")
            })
        })
}

fn check_footer(file: &mut File) -> io::Result<()> {
    let size = file.metadata()?.len();
    if size < 12 {
        return Err(invalid_data("Invalid Parquet file: missing footer"));
    }
    file.seek(SeekFrom::End(-8))?;
    let mut footer = [0; 8];
    file.read_exact(&mut footer)?;
    if &footer[4..] != b"PAR1" {
        return Err(invalid_data("Invalid or encrypted Parquet footer"));
    }
    let metadata_size = u32::from_le_bytes(footer[..4].try_into().expect("four footer bytes"));
    if u64::from(metadata_size) > MAX_METADATA_BYTES {
        return Err(invalid_data(
            "Parquet metadata exceeds the 8 MiB preview limit",
        ));
    }
    if u64::from(metadata_size) > size - 12 {
        return Err(invalid_data("Invalid Parquet metadata length"));
    }
    Ok(())
}

// Stop formatting at the limit, including for large nested or binary values,
// instead of allocating an unbounded string and truncating it afterwards.
fn cell(value: impl fmt::Display, truncated: &mut bool) -> String {
    struct Text {
        value: String,
        remaining: usize,
    }

    impl fmt::Write for Text {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            for c in text.chars() {
                if self.remaining == 0 {
                    return Err(fmt::Error);
                }
                self.value.push(c);
                self.remaining -= 1;
            }
            Ok(())
        }
    }

    let mut text = Text {
        value: String::new(),
        remaining: MAX_CELL_CHARS,
    };
    if write!(&mut text, "{value}").is_err() {
        text.value.pop();
        text.value.push('…');
        *truncated = true;
    }
    text.value
}

fn field_cell(field: &Field, truncated: &mut bool) -> String {
    struct SafeField<'a>(&'a Field);

    impl fmt::Display for SafeField<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.0 {
                Field::Date(days) => {
                    match chrono::NaiveDate::from_ymd_opt(1970, 1, 1).and_then(|epoch| {
                        epoch.checked_add_signed(chrono::Duration::days(i64::from(*days)))
                    }) {
                        Some(date) => write!(f, "{}", date.format("%Y-%m-%d")),
                        None => write!(f, "{days}"),
                    }
                }
                Field::TimestampMillis(millis) => {
                    match chrono::DateTime::<chrono::Utc>::from_timestamp_millis(*millis) {
                        Some(date) => write!(f, "{}", date.format("%Y-%m-%d %H:%M:%S%.3f %:z")),
                        None => write!(f, "{millis}"),
                    }
                }
                Field::TimestampMicros(micros) => {
                    match chrono::DateTime::<chrono::Utc>::from_timestamp_micros(*micros) {
                        Some(date) => write!(f, "{}", date.format("%Y-%m-%d %H:%M:%S%.6f %:z")),
                        None => write!(f, "{micros}"),
                    }
                }
                Field::Str(value) => f.write_str(value),
                Field::Decimal(value) => match decimal_text(value) {
                    Some(text) => f.write_str(&text),
                    // Parquet's Display asserts on invalid decimal metadata;
                    // the Debug form remains safe for malformed values.
                    None => write!(f, "{:?}", value),
                },
                Field::Group(row) => {
                    f.write_str("{")?;
                    for (index, (name, field)) in row.get_column_iter().enumerate() {
                        if index > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{name}: {}", SafeField(field))?;
                    }
                    f.write_str("}")
                }
                Field::ListInternal(list) => {
                    f.write_str("[")?;
                    for (index, field) in list.elements().iter().enumerate() {
                        if index > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{}", SafeField(field))?;
                    }
                    f.write_str("]")
                }
                Field::MapInternal(map) => {
                    f.write_str("{")?;
                    for (index, (key, value)) in map.entries().iter().enumerate() {
                        if index > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{} -> {}", SafeField(key), SafeField(value))?;
                    }
                    f.write_str("}")
                }
                _ => write!(f, "{}", self.0),
            }
        }
    }

    cell(SafeField(field), truncated)
}

fn decimal_text(decimal: &parquet::data_type::Decimal) -> Option<String> {
    let bytes = decimal.data();
    let scale = decimal.scale();
    if bytes.is_empty()
        || bytes.len() > 16
        || !(0..=38).contains(&scale)
        || decimal.precision() <= 0
        || decimal.precision() < scale
    {
        return None;
    }
    let negative = bytes[0] & 0x80 != 0;
    let mut raw = [if negative { 0xff } else { 0 }; 16];
    raw[16 - bytes.len()..].copy_from_slice(bytes);
    let value = i128::from_be_bytes(raw);
    let mut digits = value.to_string();
    if scale == 0 {
        return Some(digits);
    }
    let sign_len = usize::from(negative);
    let point = digits.len() as i32 - scale - sign_len as i32;
    if point <= 0 {
        for _ in point..0 {
            digits.insert(sign_len, '0');
        }
        digits.insert_str(sign_len, "0.");
    } else {
        digits.insert((point + sign_len as i32) as usize, '.');
    }
    Some(digits)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::path::Path;
    use std::sync::Arc;

    use parquet::basic::Compression;
    use parquet::data_type::{BoolType, ByteArray, ByteArrayType, Int32Type};
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::schema::parser::parse_message_type;
    use tempfile::TempDir;

    use super::{
        MAX_CELL_CHARS, MAX_COLS, MAX_METADATA_BYTES, MAX_ROWS, MAX_SAMPLE_BYTES, cell, field_cell,
        selected_chunk_bytes,
    };
    use crate::preview::{Content, Registry};
    use parquet::record::Field;

    fn fixture(path: &Path, groups: &[usize], compression: Compression) {
        let schema = Arc::new(parse_message_type(
            "message sample { REQUIRED INT32 id; OPTIONAL BINARY name (UTF8); REQUIRED BOOLEAN active; }",
        ).unwrap());
        let props = Arc::new(
            WriterProperties::builder()
                .set_compression(compression)
                .build(),
        );
        let mut writer =
            SerializedFileWriter::new(File::create(path).unwrap(), schema, props).unwrap();
        let mut start = 0;
        for &count in groups {
            let mut group = writer.next_row_group().unwrap();
            let mut ids = group.next_column().unwrap().unwrap();
            let values: Vec<_> = (start..start + count).map(|n| n as i32).collect();
            ids.typed::<Int32Type>()
                .write_batch(&values, None, None)
                .unwrap();
            ids.close().unwrap();
            let mut names = group.next_column().unwrap().unwrap();
            let values: Vec<_> = (0..count)
                .filter(|n| n % 2 == 0)
                .map(|_| ByteArray::from("žluťoučký"))
                .collect();
            let levels: Vec<_> = (0..count).map(|n| i16::from(n % 2 == 0)).collect();
            names
                .typed::<ByteArrayType>()
                .write_batch(&values, Some(&levels), None)
                .unwrap();
            names.close().unwrap();
            let mut active = group.next_column().unwrap().unwrap();
            active
                .typed::<BoolType>()
                .write_batch(&vec![true; count], None, None)
                .unwrap();
            active.close().unwrap();
            group.close().unwrap();
            start += count;
        }
        writer.close().unwrap();
    }

    #[test]
    fn reads_compressed_typed_rows_and_nulls() {
        let dir = TempDir::new().unwrap();
        for compression in [
            Compression::UNCOMPRESSED,
            Compression::SNAPPY,
            Compression::GZIP(Default::default()),
            Compression::ZSTD(Default::default()),
            Compression::LZ4_RAW,
            Compression::BROTLI(Default::default()),
        ] {
            // Magic detection also handles files without an extension.
            let path = dir.path().join("sample");
            fixture(&path, &[2, 1], compression);
            let preview = Registry::with_defaults().preview(&path).unwrap();
            assert_eq!(preview.kind, "Parquet");
            assert_eq!(
                preview.content,
                Content::Table {
                    header: vec!["id".into(), "name".into(), "active".into()],
                    rows: vec![
                        vec!["0".into(), "žluťoučký".into(), "true".into()],
                        vec!["1".into(), "null".into(), "true".into()],
                        vec!["2".into(), "žluťoučký".into(), "true".into()]
                    ],
                    truncated: false,
                }
            );
        }
    }

    #[test]
    fn bounds_rows_across_groups_and_handles_empty_files() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("sample.PARQUET");
        for count in [0, MAX_ROWS, MAX_ROWS + 1] {
            let groups = if count == 0 {
                vec![]
            } else {
                vec![100, count - 100]
            };
            fixture(&path, &groups, Compression::SNAPPY);
            let Content::Table {
                header,
                rows,
                truncated,
            } = Registry::with_defaults().preview(&path).unwrap().content
            else {
                panic!("expected table")
            };
            assert_eq!(header, ["id", "name", "active"]);
            assert_eq!(rows.len(), count.min(MAX_ROWS));
            assert_eq!(truncated, count > MAX_ROWS);
        }
    }

    #[test]
    fn projects_only_the_first_columns() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("wide.parquet");
        let fields = (0..MAX_COLS + 1)
            .map(|i| format!("REQUIRED INT32 c{i};"))
            .collect::<String>();
        let schema =
            Arc::new(parse_message_type(&format!("message sample {{ {fields} }}")).unwrap());
        let mut writer =
            SerializedFileWriter::new(File::create(&path).unwrap(), schema, Default::default())
                .unwrap();
        let mut group = writer.next_row_group().unwrap();
        for i in 0..MAX_COLS + 1 {
            let mut column = group.next_column().unwrap().unwrap();
            column
                .typed::<Int32Type>()
                .write_batch(&[i as i32], None, None)
                .unwrap();
            column.close().unwrap();
        }
        group.close().unwrap();
        writer.close().unwrap();
        let Content::Table {
            header,
            rows,
            truncated,
        } = Registry::with_defaults().preview(&path).unwrap().content
        else {
            panic!("expected table")
        };
        assert_eq!(header.len(), MAX_COLS);
        assert_eq!(rows[0].len(), MAX_COLS);
        assert_eq!(rows[0][MAX_COLS - 1], (MAX_COLS - 1).to_string());
        assert!(truncated);
    }

    #[test]
    fn caps_unicode_cells_without_splitting_characters() {
        let mut truncated = false;
        assert_eq!(
            cell("ž".repeat(MAX_CELL_CHARS), &mut truncated)
                .chars()
                .count(),
            MAX_CELL_CHARS
        );
        assert!(!truncated);
        let value = cell("ž".repeat(MAX_CELL_CHARS + 1), &mut truncated);
        assert!(value.ends_with('…'));
        assert_eq!(value.chars().count(), MAX_CELL_CHARS);
        assert!(truncated);
    }

    #[test]
    fn enforces_the_projected_chunk_budget_using_larger_encoded_size() {
        assert_eq!(
            selected_chunk_bytes([(MAX_SAMPLE_BYTES as i64 - 1, 1), (1, 1)]).unwrap(),
            MAX_SAMPLE_BYTES
        );
        assert!(
            selected_chunk_bytes([(MAX_SAMPLE_BYTES as i64, MAX_SAMPLE_BYTES as i64), (1, 1)])
                .unwrap()
                > MAX_SAMPLE_BYTES
        );
    }

    #[test]
    fn caps_cells_in_a_real_parquet_row() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("long.parquet");
        let schema = Arc::new(
            parse_message_type("message sample { REQUIRED BINARY name (UTF8); }").unwrap(),
        );
        let mut writer =
            SerializedFileWriter::new(File::create(&path).unwrap(), schema, Default::default())
                .unwrap();
        let mut group = writer.next_row_group().unwrap();
        let mut column = group.next_column().unwrap().unwrap();
        let value = ByteArray::from("ž".repeat(MAX_CELL_CHARS + 1).into_bytes());
        column
            .typed::<ByteArrayType>()
            .write_batch(&[value], None, None)
            .unwrap();
        column.close().unwrap();
        group.close().unwrap();
        writer.close().unwrap();

        let Content::Table {
            rows, truncated, ..
        } = Registry::with_defaults().preview(&path).unwrap().content
        else {
            panic!("expected table");
        };
        assert_eq!(rows[0][0].chars().count(), MAX_CELL_CHARS);
        assert!(rows[0][0].ends_with('…'));
        assert!(truncated);
    }

    #[test]
    fn formats_untrusted_date_values_without_panicking() {
        let mut truncated = false;
        assert_eq!(
            field_cell(&Field::Date(i32::MAX), &mut truncated),
            i32::MAX.to_string()
        );
        assert_eq!(
            field_cell(&Field::TimestampMillis(i64::MAX), &mut truncated),
            i64::MAX.to_string()
        );
        assert_eq!(
            field_cell(&Field::TimestampMicros(i64::MIN), &mut truncated),
            i64::MIN.to_string()
        );
        assert_eq!(
            field_cell(
                &Field::Group(parquet::record::Row::new(vec![(
                    "date".into(),
                    Field::Date(i32::MAX),
                )])),
                &mut truncated,
            ),
            format!("{{date: {}}}", i32::MAX)
        );
    }

    #[test]
    fn displays_decimal_values_using_their_scale() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("decimal.parquet");
        let schema = Arc::new(
            parse_message_type("message sample { REQUIRED INT32 amount (DECIMAL(2,2)); }").unwrap(),
        );
        let mut writer =
            SerializedFileWriter::new(File::create(&path).unwrap(), schema, Default::default())
                .unwrap();
        let mut group = writer.next_row_group().unwrap();
        let mut column = group.next_column().unwrap().unwrap();
        column
            .typed::<Int32Type>()
            .write_batch(&[12], None, None)
            .unwrap();
        column.close().unwrap();
        group.close().unwrap();
        writer.close().unwrap();

        let Content::Table { rows, .. } = Registry::with_defaults().preview(&path).unwrap().content
        else {
            panic!("expected table");
        };
        assert_eq!(rows[0][0], "0.12");
    }

    #[test]
    fn reports_repeated_columns_as_unsupported() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("repeated.parquet");
        let schema = Arc::new(parse_message_type(
            "message sample { REQUIRED GROUP values (LIST) { REPEATED GROUP list { REQUIRED INT32 element; } } }",
        ).unwrap());
        let writer =
            SerializedFileWriter::new(File::create(&path).unwrap(), schema, Default::default())
                .unwrap();
        writer.close().unwrap();

        let error = Registry::with_defaults().preview(&path).unwrap_err();
        assert!(error.contains("does not yet support list, map or repeated columns"));
    }

    #[test]
    fn reports_invalid_and_oversized_metadata_with_the_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.parquet");
        for bytes in [
            b"not parquet".to_vec(),
            [
                b"PAR1".as_slice(),
                &((MAX_METADATA_BYTES + 1) as u32).to_le_bytes(),
                b"PAR1",
            ]
            .concat(),
            b"PAR1\xff\xff\x00\x00PAR1".to_vec(),
        ] {
            fs::write(&path, bytes).unwrap();
            let error = Registry::with_defaults().preview(&path).unwrap_err();
            assert!(error.starts_with(&format!("{}: ", path.display())));
        }
    }
}
