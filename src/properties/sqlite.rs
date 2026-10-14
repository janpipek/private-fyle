//! Bounded, cancellable SQLite table-name inspection for local Properties.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags};

use crate::properties::{GroupId, GroupStatus, PropertyGroup, PropertyTable, PropertyValue};

const MAX_TABLES: usize = 128;
const MAX_NAME_CHARS: usize = 1024;
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

/// Reads user table names only, with a table-count, text and time limit.
pub fn read(path: &Path, cancel: Arc<AtomicBool>) -> io::Result<PropertyGroup> {
    let deadline = Instant::now() + DEADLINE;
    check(&cancel, deadline)?;
    // Validate the target before SQLite opens it. Symlinks to regular files are allowed.
    drop(open_regular(path)?);
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(db_error)?;
    conn.busy_timeout(Duration::from_millis(100))
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

    let mut statement = conn
        .prepare("SELECT name FROM main.sqlite_schema WHERE type = 'table' AND substr(name, 1, 7) != 'sqlite_' ORDER BY name LIMIT ?1")
        .map_err(db_error)
        .or_else(|error| classify(error, &cancel, deadline))?;
    let mut rows = statement
        .query([MAX_TABLES as i64 + 1])
        .map_err(db_error)
        .or_else(|error| classify(error, &cancel, deadline))?;
    let mut names = Vec::new();
    let mut tables_truncated = false;
    let mut text_truncated = false;
    while let Some(row) = rows
        .next()
        .map_err(db_error)
        .or_else(|error| classify(error, &cancel, deadline))?
    {
        check(&cancel, deadline)?;
        if names.len() == MAX_TABLES {
            tables_truncated = true;
            break;
        }
        let name: String = row.get(0).map_err(db_error)?;
        if name.chars().count() > MAX_NAME_CHARS {
            text_truncated = true;
            names.push(format!(
                "{}…",
                name.chars().take(MAX_NAME_CHARS - 1).collect::<String>()
            ));
        } else {
            names.push(name);
        }
    }
    check(&cancel, deadline)?;
    let mut reasons = Vec::new();
    if tables_truncated {
        reasons.push(format!("tables limited to {MAX_TABLES}"));
    }
    if text_truncated {
        reasons.push(format!("names limited to {MAX_NAME_CHARS} characters"));
    }
    Ok(PropertyGroup {
        id: GroupId("sqlite".into()),
        status: if reasons.is_empty() {
            GroupStatus::Available
        } else {
            GroupStatus::Truncated {
                reason: format!("Truncated: {}", reasons.join("; ")),
            }
        },
        fields: Vec::new(),
        groups: Vec::new(),
        tables: if names.is_empty() {
            Vec::new()
        } else {
            vec![PropertyTable {
                id: "tables".into(),
                columns: vec!["name".into()],
                rows: names
                    .into_iter()
                    .map(|name| vec![PropertyValue::Text(name)])
                    .collect(),
            }]
        },
    })
}

fn open_regular(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "linux")]
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

    use super::{MAX_TABLES, is_candidate, read};
    use crate::properties::{GroupStatus, PropertyValue};

    #[test]
    fn lists_only_user_table_names_and_leaves_database_unchanged() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("schema.sqlite");
        let conn = Connection::open(&path).expect("create database");
        conn.execute_batch("CREATE TABLE \"odd table\" (base INTEGER, value INTEGER GENERATED ALWAYS AS (base + 1) VIRTUAL); CREATE INDEX ix ON \"odd table\"(base); CREATE VIEW v AS SELECT value FROM \"odd table\"; INSERT INTO \"odd table\" (base) VALUES (41);").expect("fixture schema");
        drop(conn);
        let before = fs::read(&path).expect("read database");
        let group = read(&path, Arc::new(AtomicBool::new(false))).expect("inspect schema");
        assert_eq!(group.status, GroupStatus::Available);
        assert!(group.groups.is_empty());
        assert_eq!(group.tables[0].columns, ["name"]);
        assert_eq!(
            group.tables[0].rows,
            [[PropertyValue::Text("odd table".into())]]
        );
        assert!(is_candidate(&path));
        assert_eq!(fs::read(&path).expect("read database"), before);
    }

    #[test]
    fn empty_database_has_no_visible_sqlite_group() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("empty.sqlite");
        drop(Connection::open(&path).expect("create database"));
        let group = read(&path, Arc::new(AtomicBool::new(false))).expect("inspect database");
        assert!(group.tables.is_empty());
        assert!(!group.is_visible());
    }

    #[test]
    fn caps_table_count_and_name_length() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("schema.data");
        let conn = Connection::open(&path).expect("database");
        let long_name = format!("a{}", "x".repeat(1100));
        conn.execute_batch(&format!("CREATE TABLE \"{long_name}\" (value TEXT);"))
            .expect("long table name");
        for index in 0..MAX_TABLES {
            conn.execute_batch(&format!("CREATE TABLE t{index} (value INTEGER);"))
                .expect("table");
        }
        drop(conn);
        assert!(is_candidate(&path));
        let group = read(&path, Arc::new(AtomicBool::new(false))).expect("read");
        assert_eq!(group.tables[0].rows.len(), MAX_TABLES);
        assert!(
            matches!(group.status, GroupStatus::Truncated { ref reason } if reason.contains("tables limited") && reason.contains("names limited"))
        );
        assert!(group.tables[0].rows.iter().all(|row| match &row[0] {
            PropertyValue::Text(name) => name.chars().count() <= 1024,
            _ => false,
        }));
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
}
