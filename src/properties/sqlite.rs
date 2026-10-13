//! Bounded, cancellable SQLite schema inspection for local Properties.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rusqlite::limits::Limit;
use rusqlite::{Connection, OpenFlags, Row};

use crate::properties::{GroupId, GroupStatus, PropertyGroup, PropertyTable, PropertyValue};

const MAX_OBJECTS: usize = 128;
const MAX_DETAIL_ROWS: usize = 1024;
const MAX_TEXT_CHARS: usize = 1024;
const DEADLINE: Duration = Duration::from_secs(2);
const HEADER: &[u8; 16] = b"SQLite format 3\0";

/// Reports SQLite-looking local regular files by extension or file signature.
pub fn is_candidate(path: &Path) -> bool {
    if !path.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return false;
    }
    if path.extension().is_some_and(|extension| {
        ["sqlite", "sqlite3", "db"]
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    }) {
        return true;
    }
    let Ok(mut file) = open_regular(path) else {
        return false;
    };
    let mut header = [0; 16];
    file.read_exact(&mut header).is_ok() && &header == HEADER
}

/// Reads only schema metadata, with global object and detail-row limits.
pub fn read(path: &Path, cancel: Arc<AtomicBool>) -> io::Result<PropertyGroup> {
    check(&cancel, Instant::now() + DEADLINE)?;
    // Validate the target before SQLite opens it. Symlinks to regular files are allowed.
    let file = open_regular(path)?;
    drop(file);
    let deadline = Instant::now() + DEADLINE;
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(db_error)?;
    conn.busy_timeout(Duration::from_millis(100))
        .map_err(db_error)?;
    conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, 1024 * 1024)
        .map_err(db_error)?;
    conn.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 1024 * 1024)
        .map_err(db_error)?;
    conn.set_limit(Limit::SQLITE_LIMIT_COLUMN, 256)
        .map_err(db_error)?;
    let signal = Arc::clone(&cancel);
    conn.progress_handler(
        1000,
        Some(move || signal.load(Ordering::Relaxed) || Instant::now() >= deadline),
    )
    .map_err(db_error)?;
    conn.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; BEGIN DEFERRED")
        .map_err(db_error)
        .or_else(|error| classify(error, &cancel, deadline))?;

    let mut limits = Limits::default();
    let objects = objects(&conn, &cancel, deadline, &mut limits)
        .or_else(|error| classify(error, &cancel, deadline))?;
    let mut group = PropertyGroup {
        id: GroupId("sqlite".into()),
        status: GroupStatus::Available,
        fields: Vec::new(),
        groups: Vec::new(),
        tables: vec![PropertyTable {
            id: "objects".into(),
            columns: vec!["type".into(), "name".into(), "table".into()],
            rows: objects
                .iter()
                .map(|object| {
                    vec![
                        value(&object.kind, &mut limits),
                        value(&object.name, &mut limits),
                        value(&object.table, &mut limits),
                    ]
                })
                .collect(),
        }],
    };
    let mut columns = section(
        "columns",
        &[
            "table",
            "name",
            "type",
            "not_null",
            "default",
            "primary_key",
            "hidden",
        ],
    );
    let mut indexes = section(
        "indexes",
        &[
            "table",
            "name",
            "unique",
            "origin",
            "partial",
            "column",
            "descending",
            "collation",
            "key",
        ],
    );
    let mut foreign_keys = section(
        "foreign_keys",
        &[
            "table",
            "id",
            "sequence",
            "references",
            "from",
            "to",
            "on_update",
            "on_delete",
            "match",
        ],
    );

    for object in &objects {
        check(&cancel, deadline)?;
        if object.kind != "table" && object.kind != "view" {
            continue;
        }
        if columns.status == GroupStatus::Available
            && let Err(error) =
                read_columns(&conn, object, &cancel, deadline, &mut limits, &mut columns)
        {
            if error.kind() == io::ErrorKind::Interrupted || error.kind() == io::ErrorKind::TimedOut
            {
                return classify(error, &cancel, deadline);
            }
            columns.status = GroupStatus::Failed(error.to_string());
        }
        if object.kind == "table" {
            if indexes.status == GroupStatus::Available
                && let Err(error) =
                    read_indexes(&conn, object, &cancel, deadline, &mut limits, &mut indexes)
            {
                if error.kind() == io::ErrorKind::Interrupted
                    || error.kind() == io::ErrorKind::TimedOut
                {
                    return classify(error, &cancel, deadline);
                }
                indexes.status = GroupStatus::Failed(error.to_string());
            }
            if foreign_keys.status == GroupStatus::Available
                && let Err(error) = read_foreign_keys(
                    &conn,
                    object,
                    &cancel,
                    deadline,
                    &mut limits,
                    &mut foreign_keys,
                )
            {
                if error.kind() == io::ErrorKind::Interrupted
                    || error.kind() == io::ErrorKind::TimedOut
                {
                    return classify(error, &cancel, deadline);
                }
                foreign_keys.status = GroupStatus::Failed(error.to_string());
            }
        }
    }
    check(&cancel, deadline)?;
    let mut reasons = Vec::new();
    if limits.objects {
        reasons.push(format!("objects limited to {MAX_OBJECTS}"));
    }
    if limits.details_truncated {
        reasons.push(format!("detail rows limited to {MAX_DETAIL_ROWS}"));
    }
    if limits.text {
        reasons.push(format!("text limited to {MAX_TEXT_CHARS} characters"));
    }
    if !reasons.is_empty() {
        group.status = GroupStatus::Truncated {
            reason: format!("Truncated: {}", reasons.join("; ")),
        };
    }
    group.groups = [columns, indexes, foreign_keys]
        .into_iter()
        .filter(|section| {
            section.status != GroupStatus::Available
                || section.tables.iter().any(|table| !table.rows.is_empty())
        })
        .collect();
    Ok(group)
}

#[derive(Default)]
struct Limits {
    objects: bool,
    details: usize,
    details_truncated: bool,
    text: bool,
}

struct Object {
    kind: String,
    name: String,
    table: String,
}

fn objects(
    conn: &Connection,
    cancel: &AtomicBool,
    deadline: Instant,
    limits: &mut Limits,
) -> io::Result<Vec<Object>> {
    let mut statement = conn.prepare("SELECT type, name, tbl_name FROM main.sqlite_schema WHERE type IN ('table', 'view', 'index') AND substr(name, 1, 7) != 'sqlite_' ORDER BY CASE type WHEN 'table' THEN 0 WHEN 'view' THEN 1 ELSE 2 END, name LIMIT ?1").map_err(db_error)?;
    let mut rows = statement
        .query([MAX_OBJECTS as i64 + 1])
        .map_err(db_error)?;
    let mut objects = Vec::new();
    while let Some(row) = rows.next().map_err(db_error)? {
        check(cancel, deadline)?;
        if objects.len() == MAX_OBJECTS {
            limits.objects = true;
            break;
        }
        objects.push(Object {
            kind: row.get(0).map_err(db_error)?,
            name: row.get(1).map_err(db_error)?,
            table: row.get(2).map_err(db_error)?,
        });
    }
    Ok(objects)
}

fn section(id: &str, columns: &[&str]) -> PropertyGroup {
    PropertyGroup {
        id: GroupId(id.into()),
        status: GroupStatus::Available,
        fields: Vec::new(),
        groups: Vec::new(),
        tables: vec![PropertyTable {
            id: id.into(),
            columns: columns.iter().map(|column| (*column).into()).collect(),
            rows: Vec::new(),
        }],
    }
}

fn read_columns(
    conn: &Connection,
    object: &Object,
    cancel: &AtomicBool,
    deadline: Instant,
    limits: &mut Limits,
    section: &mut PropertyGroup,
) -> io::Result<()> {
    let mut statement = conn.prepare("SELECT name, type, \"notnull\", dflt_value, pk, hidden FROM pragma_table_xinfo(?1, 'main') ORDER BY cid").map_err(db_error)?;
    let mut rows = statement.query([&object.name]).map_err(db_error)?;
    while let Some(row) = rows.next().map_err(db_error)? {
        check(cancel, deadline)?;
        if !reserve_detail(limits) {
            break;
        }
        section.tables[0].rows.push(vec![
            value(&object.name, limits),
            cell(row, 0, limits)?,
            cell(row, 1, limits)?,
            PropertyValue::Boolean(row.get::<_, i64>(2).map_err(db_error)? != 0),
            cell(row, 3, limits)?,
            PropertyValue::Signed(row.get(4).map_err(db_error)?),
            PropertyValue::Signed(row.get(5).map_err(db_error)?),
        ]);
    }
    Ok(())
}

fn read_indexes(
    conn: &Connection,
    object: &Object,
    cancel: &AtomicBool,
    deadline: Instant,
    limits: &mut Limits,
    section: &mut PropertyGroup,
) -> io::Result<()> {
    let mut statement = conn.prepare("SELECT name, \"unique\", origin, partial FROM pragma_index_list(?1, 'main') ORDER BY seq").map_err(db_error)?;
    let mut rows = statement.query([&object.name]).map_err(db_error)?;
    while let Some(row) = rows.next().map_err(db_error)? {
        check(cancel, deadline)?;
        let name: String = row.get(0).map_err(db_error)?;
        let unique: i64 = row.get(1).map_err(db_error)?;
        let origin: String = row.get(2).map_err(db_error)?;
        let partial: i64 = row.get(3).map_err(db_error)?;
        let mut columns = conn.prepare("SELECT name, \"desc\", coll, \"key\" FROM pragma_index_xinfo(?1, 'main') ORDER BY seqno").map_err(db_error)?;
        let mut column_rows = columns.query([&name]).map_err(db_error)?;
        let mut found_column = false;
        while let Some(column) = column_rows.next().map_err(db_error)? {
            check(cancel, deadline)?;
            found_column = true;
            if !reserve_detail(limits) {
                break;
            }
            section.tables[0].rows.push(vec![
                value(&object.name, limits),
                value(&name, limits),
                PropertyValue::Boolean(unique != 0),
                value(&origin, limits),
                PropertyValue::Boolean(partial != 0),
                cell(column, 0, limits)?,
                PropertyValue::Boolean(column.get::<_, i64>(1).map_err(db_error)? != 0),
                cell(column, 2, limits)?,
                PropertyValue::Boolean(column.get::<_, i64>(3).map_err(db_error)? != 0),
            ]);
        }
        if !found_column && reserve_detail(limits) {
            section.tables[0].rows.push(vec![
                value(&object.name, limits),
                value(&name, limits),
                PropertyValue::Boolean(unique != 0),
                value(&origin, limits),
                PropertyValue::Boolean(partial != 0),
                value("—", limits),
                PropertyValue::Boolean(false),
                value("—", limits),
                PropertyValue::Boolean(false),
            ]);
        }
    }
    Ok(())
}

fn read_foreign_keys(
    conn: &Connection,
    object: &Object,
    cancel: &AtomicBool,
    deadline: Instant,
    limits: &mut Limits,
    section: &mut PropertyGroup,
) -> io::Result<()> {
    let mut statement = conn.prepare("SELECT id, seq, \"table\", \"from\", \"to\", on_update, on_delete, \"match\" FROM pragma_foreign_key_list(?1, 'main') ORDER BY id, seq").map_err(db_error)?;
    let mut rows = statement.query([&object.name]).map_err(db_error)?;
    while let Some(row) = rows.next().map_err(db_error)? {
        check(cancel, deadline)?;
        if !reserve_detail(limits) {
            break;
        }
        section.tables[0].rows.push(vec![
            value(&object.name, limits),
            PropertyValue::Signed(row.get(0).map_err(db_error)?),
            PropertyValue::Signed(row.get(1).map_err(db_error)?),
            cell(row, 2, limits)?,
            cell(row, 3, limits)?,
            cell(row, 4, limits)?,
            cell(row, 5, limits)?,
            cell(row, 6, limits)?,
            cell(row, 7, limits)?,
        ]);
    }
    Ok(())
}

fn reserve_detail(limits: &mut Limits) -> bool {
    if limits.details == MAX_DETAIL_ROWS {
        limits.details_truncated = true;
        false
    } else {
        limits.details += 1;
        true
    }
}

fn cell(row: &Row<'_>, index: usize, limits: &mut Limits) -> io::Result<PropertyValue> {
    let text: Option<String> = row.get(index).map_err(db_error)?;
    Ok(value(text.as_deref().unwrap_or("—"), limits))
}

fn value(text: &str, limits: &mut Limits) -> PropertyValue {
    if text.chars().count() > MAX_TEXT_CHARS {
        limits.text = true;
        PropertyValue::Text(text.chars().take(MAX_TEXT_CHARS - 1).chain(['…']).collect())
    } else {
        PropertyValue::Text(text.into())
    }
}

fn open_regular(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SQLite Properties requires a local regular file",
        ));
    }
    Ok(file)
}

fn check(cancel: &AtomicBool, deadline: Instant) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "SQLite inspection cancelled",
        ))
    } else if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "SQLite inspection timed out",
        ))
    } else {
        Ok(())
    }
}

fn classify<T>(error: io::Error, cancel: &AtomicBool, deadline: Instant) -> io::Result<T> {
    check(cancel, deadline)?;
    Err(error)
}

fn db_error(error: rusqlite::Error) -> io::Error {
    let kind = match &error {
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == rusqlite::ErrorCode::OperationInterrupted =>
        {
            io::ErrorKind::Interrupted
        }
        _ => io::ErrorKind::InvalidData,
    };
    io::Error::new(kind, error)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use rusqlite::Connection;
    use tempfile::TempDir;

    use super::{MAX_OBJECTS, is_candidate, read};
    use crate::properties::{GroupStatus, PropertyValue};

    #[test]
    fn reads_quoted_schema_without_reading_user_rows_or_changing_file() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("schema.sqlite");
        let conn = Connection::open(&path).expect("create database");
        conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE \"odd' parent\" (\"id\" INTEGER PRIMARY KEY); CREATE TABLE \"child table\" (\"a b\" TEXT, \"computed\" TEXT GENERATED ALWAYS AS (\"a b\" || 'x') VIRTUAL, \"ref\" INTEGER REFERENCES \"odd' parent\"(id)); CREATE INDEX \"ix odd\" ON \"child table\"(\"a b\" DESC); CREATE VIEW \"v odd\" AS SELECT \"a b\" FROM \"child table\"; INSERT INTO \"child table\" (\"a b\") VALUES ('secret-data');").expect("fixture schema");
        drop(conn);
        let before = fs::read(&path).expect("read database");
        let group = read(&path, Arc::new(AtomicBool::new(false))).expect("inspect schema");
        assert!(matches!(group.status, GroupStatus::Available));
        assert!(is_candidate(&path));
        let objects = &group.tables[0].rows;
        assert!(
            objects
                .iter()
                .any(|row| row[1] == PropertyValue::Text("child table".into()))
        );
        assert!(
            objects
                .iter()
                .any(|row| row[1] == PropertyValue::Text("v odd".into()))
        );
        let columns = &group.groups[0].tables[0].rows;
        assert!(
            columns
                .iter()
                .any(|row| row[1] == PropertyValue::Text("computed".into())
                    && row[6] == PropertyValue::Signed(2))
        );
        let indexes = &group.groups[1].tables[0].rows;
        assert!(
            indexes
                .iter()
                .any(|row| row[1] == PropertyValue::Text("ix odd".into())
                    && row[6] == PropertyValue::Boolean(true))
        );
        let keys = &group.groups[2].tables[0].rows;
        assert!(
            keys.iter()
                .any(|row| row[3] == PropertyValue::Text("odd' parent".into()))
        );
        assert!(!format!("{group:?}").contains("secret-data"));
        assert_eq!(fs::read(&path).expect("read database"), before);
    }

    #[test]
    fn rejects_malformed_database_and_precancellation() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("bad.db");
        fs::write(&path, b"bad database").expect("write fixture");
        assert!(is_candidate(&path));
        assert!(read(&path, Arc::new(AtomicBool::new(false))).is_err());
        assert_eq!(
            read(&path, Arc::new(AtomicBool::new(true)))
                .expect_err("cancelled")
                .kind(),
            io::ErrorKind::Interrupted
        );
    }

    #[test]
    fn detects_header_and_caps_objects_and_text() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("schema.data");
        let conn = Connection::open(&path).expect("database");
        let long_name = format!("a{}", "x".repeat(1100));
        conn.execute_batch(&format!("CREATE TABLE \"{long_name}\" (value TEXT);"))
            .expect("long name");
        for index in 0..MAX_OBJECTS {
            conn.execute_batch(&format!("CREATE TABLE t{index} (value INTEGER);"))
                .expect("table");
        }
        drop(conn);
        assert!(is_candidate(&path));
        let group = read(&path, Arc::new(AtomicBool::new(false))).expect("read");
        assert_eq!(group.tables[0].rows.len(), MAX_OBJECTS);
        assert!(
            matches!(group.status, GroupStatus::Truncated { ref reason } if reason.contains("objects limited") && reason.contains("text limited"))
        );
    }

    #[test]
    fn caps_detail_rows_across_tables() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("wide.sqlite");
        let conn = Connection::open(&path).expect("database");
        for table in 0..7 {
            let columns = (0..160)
                .map(|column| format!("c{column} TEXT"))
                .collect::<Vec<_>>()
                .join(",");
            conn.execute_batch(&format!("CREATE TABLE t{table} ({columns});"))
                .expect("wide table");
        }
        drop(conn);
        let group = read(&path, Arc::new(AtomicBool::new(false))).expect("read");
        assert!(
            matches!(group.status, GroupStatus::Truncated { ref reason } if reason.contains("detail rows limited"))
        );
        assert_eq!(group.groups[0].tables[0].rows.len(), 1024);
    }
}
