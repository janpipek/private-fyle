//! Bounded spreadsheet sheet samples (XLSX, XLSM, XLS, XLSB, ODS) through
//! `calamine`, rendered by the table preview one sheet at a time.

use std::fs::File;
use std::io;
use std::path::Path;

use calamine::{Data, Reader, open_workbook_auto};

use super::{Content, Previewer, Probe};

const MAX_ROWS: usize = 500;
const MAX_COLS: usize = 40;
const MAX_CELL_CHARS: usize = 1024;
/// Workbooks are decoded in memory, so larger files are refused.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// Declared uncompressed size of all parts of a zip-based workbook.
const MAX_UNCOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;

const EXTENSIONS: &[&str] = &["xlsx", "xlsm", "xls", "xlsb", "ods"];

/// One sheet's bounded sample: a row-number column plus up to [`MAX_COLS`]
/// data columns headed by their column letters.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sheet {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// Rows and columns of the sheet's used range, before truncation.
    pub total_rows: usize,
    pub total_cols: usize,
    /// Rows, columns or cell text were cut.
    pub truncated: bool,
}

impl Sheet {
    /// One-line description for the footer, e.g. "500 of 1200 rows, 40 of 52
    /// columns (sample truncated)".
    pub fn summary(&self) -> String {
        if self.total_rows == 0 {
            return "Empty sheet".into();
        }
        let part = |shown: usize, total: usize, unit: &str| {
            if shown < total {
                format!("{shown} of {total} {unit}")
            } else {
                format!("{total} {unit}")
            }
        };
        let cols = self.total_cols.min(MAX_COLS);
        format!(
            "{}, {}{}",
            part(self.rows.len(), self.total_rows, "rows"),
            part(cols, self.total_cols, "columns"),
            if self.truncated {
                " (sample truncated)"
            } else {
                ""
            }
        )
    }
}

/// Native spreadsheet preview of the first sheet; the view loads others on
/// demand with [`read_sheet`].
pub struct SpreadsheetPreviewer;

impl Previewer for SpreadsheetPreviewer {
    fn name(&self) -> &'static str {
        "Spreadsheet"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && EXTENSIONS.contains(&probe.ext.as_str())
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        let mut workbook = open(&probe.path)?;
        let sheets = workbook.sheet_names();
        if sheets.is_empty() {
            return Err(invalid_data("the workbook has no sheets"));
        }
        let first = sheet_of(&mut workbook, &sheets[0])?;
        Ok(Content::Spreadsheet {
            path: probe.path.clone(),
            sheets,
            first: Box::new(first),
        })
    }
}

/// Reads the sheet at `index` of the workbook at `path`. Blocking.
pub fn read_sheet(path: &Path, index: usize) -> io::Result<Sheet> {
    let mut workbook = open(path)?;
    let name = workbook
        .sheet_names()
        .get(index)
        .cloned()
        .ok_or_else(|| invalid_data("no such sheet"))?;
    sheet_of(&mut workbook, &name)
}

fn invalid_data(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn open(path: &Path) -> io::Result<calamine::Sheets<std::io::BufReader<File>>> {
    let size = std::fs::metadata(path)?.len();
    if size > MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "spreadsheet exceeds the 32 MiB preview limit",
        ));
    }
    check_zip_size(path)?;
    open_workbook_auto(path).map_err(invalid_data)
}

/// Refuses zip-based workbooks that declare an excessive uncompressed size.
/// Other formats (and invalid zips, which calamine reports) pass.
fn check_zip_size(path: &Path) -> io::Result<()> {
    let Ok(mut archive) = zip::ZipArchive::new(File::open(path)?) else {
        return Ok(());
    };
    let mut total = 0u64;
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index_raw(i) {
            total = total.saturating_add(entry.size());
        }
    }
    if total > MAX_UNCOMPRESSED_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "spreadsheet exceeds the 256 MiB uncompressed preview limit",
        ));
    }
    Ok(())
}

fn sheet_of<RS: io::Read + io::Seek>(
    workbook: &mut calamine::Sheets<RS>,
    name: &str,
) -> io::Result<Sheet> {
    let range = workbook.worksheet_range(name).map_err(invalid_data)?;
    let Some((row0, col0)) = range.start() else {
        return Ok(Sheet::default());
    };
    let (total_rows, total_cols) = range.get_size();
    let mut truncated = total_rows > MAX_ROWS || total_cols > MAX_COLS;
    let shown_cols = total_cols.min(MAX_COLS);
    let mut header = vec![String::new()];
    header.extend((0..shown_cols).map(|i| column_letters(col0 as usize + i)));
    let rows = range
        .rows()
        .take(MAX_ROWS)
        .enumerate()
        .map(|(i, row)| {
            let mut out = Vec::with_capacity(shown_cols + 1);
            out.push((row0 as usize + i + 1).to_string());
            out.extend(row.iter().take(MAX_COLS).map(|data| {
                let (text, cut) = cap(format_cell(data));
                truncated |= cut;
                text
            }));
            out
        })
        .collect();
    Ok(Sheet {
        header,
        rows,
        total_rows,
        total_cols,
        truncated,
    })
}

/// Spreadsheet column name for a zero-based index: A, B, …, Z, AA, ….
pub fn column_letters(mut index: usize) -> String {
    let mut letters = Vec::new();
    loop {
        letters.push(b'A' + (index % 26) as u8);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    letters.reverse();
    String::from_utf8(letters).expect("ASCII letters")
}

/// Cuts text to [`MAX_CELL_CHARS`] characters; reports whether it did.
fn cap(text: String) -> (String, bool) {
    match text.char_indices().nth(MAX_CELL_CHARS) {
        Some((end, _)) => (format!("{}…", &text[..end]), true),
        None => (text, false),
    }
}

/// A cell as text: integers without ".0", dates as ISO text, errors as codes.
fn format_cell(data: &Data) -> String {
    match data {
        Data::Empty => String::new(),
        Data::String(text) => text.clone(),
        Data::Int(n) => n.to_string(),
        Data::Float(f) => format_float(*f),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Data::Error(error) => error.to_string(),
        Data::DateTimeIso(text) | Data::DurationIso(text) => text.clone(),
        Data::DateTime(date) => {
            if date.is_duration() {
                match date.as_duration() {
                    Some(duration) => format_duration(duration.num_seconds()),
                    None => format_float(date.as_f64()),
                }
            } else {
                match date.as_datetime() {
                    // Values below one day are times of day.
                    Some(_) if date.as_f64() >= 0.0 && date.as_f64() < 1.0 => date
                        .as_datetime()
                        .map(|d| d.format("%H:%M:%S").to_string())
                        .unwrap_or_default(),
                    Some(dt) if dt.time() == chrono::NaiveTime::MIN => {
                        dt.format("%Y-%m-%d").to_string()
                    }
                    Some(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
                    None => format_float(date.as_f64()),
                }
            }
        }
    }
}

fn format_float(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
}

fn format_duration(seconds: i64) -> String {
    let sign = if seconds < 0 { "-" } else { "" };
    let s = seconds.unsigned_abs();
    format!("{sign}{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    use super::{
        Content, MAX_CELL_CHARS, MAX_COLS, MAX_ROWS, Previewer, Probe, SpreadsheetPreviewer,
        column_letters, format_float, read_sheet,
    };

    fn zip_file(dir: &TempDir, name: &str, parts: &[(&str, &str)]) -> PathBuf {
        let path = dir.path().join(name);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (part, body) in parts {
            zip.start_file(*part, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    /// A minimal XLSX with the given sheet bodies (`<row>` elements).
    fn xlsx(dir: &TempDir, sheets: &[(&str, String)]) -> PathBuf {
        let mut parts: Vec<(String, String)> = vec![
            (
                "[Content_Types].xml".into(),
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/></Types>"#.into(),
            ),
            (
                "_rels/.rels".into(),
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.into(),
            ),
            (
                "xl/styles.xml".into(),
                r#"<?xml version="1.0"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cellXfs count="3"><xf numFmtId="0"/><xf numFmtId="14"/><xf numFmtId="22"/></cellXfs></styleSheet>"#.into(),
            ),
        ];
        let mut book = String::from(
            r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>"#,
        );
        let mut rels = String::from(
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
        );
        for (i, (name, rows)) in sheets.iter().enumerate() {
            let n = i + 1;
            book += &format!(r#"<sheet name="{name}" sheetId="{n}" r:id="rId{n}"/>"#);
            rels += &format!(
                r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#
            );
            parts.push((
                format!("xl/worksheets/sheet{n}.xml"),
                format!(
                    r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{rows}</sheetData></worksheet>"#
                ),
            ));
        }
        book += "</sheets></workbook>";
        rels += "</Relationships>";
        parts.push(("xl/workbook.xml".into(), book));
        parts.push(("xl/_rels/workbook.xml.rels".into(), rels));
        let refs: Vec<(&str, &str)> = parts
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        zip_file(dir, "book.xlsx", &refs)
    }

    fn probe(path: &Path) -> Probe {
        Probe::read(path).unwrap()
    }

    fn load(path: &Path) -> std::io::Result<Content> {
        SpreadsheetPreviewer.load(&probe(path))
    }

    const MIXED: &str = r#"<row r="3"><c r="B3" t="inlineStr"><is><t>name</t></is></c><c r="C3"><v>3</v></c><c r="D3"><v>2.5</v></c><c r="E3" t="b"><v>1</v></c><c r="F3" t="e"><v>#DIV/0!</v></c></row>
<row r="4"><c r="B4" s="1"><v>45356</v></c><c r="C4" s="2"><v>45356.5</v></c><c r="D4"><f>C3*2</f><v>6</v></c><c r="E4" t="str"><f>"a"&amp;"b"</f><v>ab</v></c></row>"#;

    #[test]
    fn lists_sheets_and_formats_cells() {
        let dir = TempDir::new().unwrap();
        let path = xlsx(
            &dir,
            &[("Data", MIXED.to_string()), ("Other", String::new())],
        );
        let Content::Spreadsheet { sheets, first, .. } = load(&path).unwrap() else {
            panic!("expected spreadsheet");
        };
        assert_eq!(sheets, ["Data", "Other"]);
        // Used range starts at B3: letters and row numbers are absolute.
        assert_eq!(first.header, ["", "B", "C", "D", "E", "F"]);
        assert_eq!(first.rows[0], ["3", "name", "3", "2.5", "TRUE", "#DIV/0!"]);
        assert_eq!(
            first.rows[1],
            ["4", "2024-03-05", "2024-03-05 12:00:00", "6", "ab", ""]
        );
        assert_eq!(first.summary(), "2 rows, 5 columns");
        assert!(!first.truncated);
    }

    #[test]
    fn reads_other_sheets_and_empty_sheet() {
        let dir = TempDir::new().unwrap();
        let path = xlsx(
            &dir,
            &[("Data", MIXED.to_string()), ("Other", String::new())],
        );
        let empty = read_sheet(&path, 1).unwrap();
        assert!(empty.rows.is_empty());
        assert_eq!(empty.summary(), "Empty sheet");
        assert_eq!(read_sheet(&path, 0).unwrap().rows.len(), 2);
        assert!(read_sheet(&path, 2).is_err());
    }

    #[test]
    fn bounds_rows_columns_and_cell_text() {
        let dir = TempDir::new().unwrap();
        let long = "x".repeat(MAX_CELL_CHARS + 10);
        let mut rows = String::new();
        for r in 1..=MAX_ROWS + 5 {
            rows += &format!(r#"<row r="{r}">"#);
            for c in 0..MAX_COLS + 3 {
                let col = column_letters(c);
                if r == 1 && c == 0 {
                    rows += &format!(r#"<c r="A1" t="inlineStr"><is><t>{long}</t></is></c>"#);
                } else {
                    rows += &format!(r#"<c r="{col}{r}"><v>{c}</v></c>"#);
                }
            }
            rows += "</row>";
        }
        let path = xlsx(&dir, &[("Big", rows)]);
        let sheet = read_sheet(&path, 0).unwrap();
        assert_eq!(sheet.rows.len(), MAX_ROWS);
        assert_eq!(sheet.header.len(), MAX_COLS + 1);
        assert_eq!(sheet.rows[0].len(), MAX_COLS + 1);
        assert_eq!(sheet.rows[0][1].chars().count(), MAX_CELL_CHARS + 1);
        assert!(sheet.rows[0][1].ends_with('…'));
        assert!(sheet.truncated);
        assert_eq!(
            sheet.summary(),
            "500 of 505 rows, 40 of 43 columns (sample truncated)"
        );
    }

    #[test]
    fn ods_is_read() {
        let dir = TempDir::new().unwrap();
        let content = r#"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet>
<table:table table:name="One"><table:table-row>
<table:table-cell office:value-type="string"><text:p>hi</text:p></table:table-cell>
<table:table-cell office:value-type="float" office:value="4"><text:p>4</text:p></table:table-cell>
<table:table-cell office:value-type="float" office:value="1.5"><text:p>1.5</text:p></table:table-cell>
<table:table-cell office:value-type="boolean" office:boolean-value="true"><text:p>TRUE</text:p></table:table-cell>
<table:table-cell office:value-type="date" office:date-value="2024-03-05"><text:p>x</text:p></table:table-cell>
</table:table-row></table:table>
<table:table table:name="Two"><table:table-row><table:table-cell office:value-type="string"><text:p>b</text:p></table:table-cell></table:table-row></table:table>
</office:spreadsheet></office:body></office:document-content>"#;
        let content = content.replace('\n', "");
        let path = zip_file(
            &dir,
            "book.ods",
            &[
                ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
                (
                    "META-INF/manifest.xml",
                    r#"<?xml version="1.0"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#,
                ),
                ("content.xml", content.as_str()),
            ],
        );
        let Content::Spreadsheet { sheets, first, .. } = load(&path).unwrap() else {
            panic!("expected spreadsheet");
        };
        assert_eq!(sheets, ["One", "Two"]);
        assert_eq!(first.rows[0], ["1", "hi", "4", "1.5", "TRUE", "2024-03-05"]);
        assert_eq!(read_sheet(&path, 1).unwrap().rows[0], ["1", "b"]);
    }

    #[test]
    fn corrupt_files_report_an_error() {
        let dir = TempDir::new().unwrap();
        for name in ["bad.xlsx", "bad.xls", "bad.ods", "bad.xlsb"] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"this is not a workbook").unwrap();
            let error = load(&path).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{name}");
            assert!(!error.to_string().is_empty());
        }
    }

    #[test]
    fn registry_prefers_it_over_the_zip_listing() {
        let dir = TempDir::new().unwrap();
        let path = xlsx(&dir, &[("Data", MIXED.to_string())]);
        let preview = crate::preview::Registry::with_defaults()
            .preview(&path)
            .unwrap();
        assert_eq!(preview.kind, "Spreadsheet");
    }

    #[test]
    fn matches_by_extension() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a.XLSM");
        std::fs::write(&path, b"x").unwrap();
        assert!(SpreadsheetPreviewer.matches(&probe(&path)));
        let path = dir.path().join("a.csv");
        std::fs::write(&path, b"x").unwrap();
        assert!(!SpreadsheetPreviewer.matches(&probe(&path)));
    }

    #[test]
    fn column_letters_and_floats() {
        assert_eq!(column_letters(0), "A");
        assert_eq!(column_letters(25), "Z");
        assert_eq!(column_letters(26), "AA");
        assert_eq!(column_letters(701), "ZZ");
        assert_eq!(column_letters(702), "AAA");
        assert_eq!(format_float(3.0), "3");
        assert_eq!(format_float(-0.5), "-0.5");
        assert_eq!(format_float(f64::NAN), "NaN");
    }
}
